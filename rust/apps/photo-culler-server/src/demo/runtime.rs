//! Executable demo router assembled from the checked-in API and fixture contracts.

use std::{
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
    path::Path,
    sync::Arc,
};

use axum::Router;
use serde::Deserialize;

use crate::{
    demo::{gallery, health, thumbnail, web_assets},
    origin::OriginPolicy,
};

const GALLERY_MANIFEST: &str = include_str!("../../../../../migration/demo/fixture/manifest.json");
const THUMBNAIL_BYTES: &[u8] =
    include_bytes!("../../../../../migration/demo/fixture/demo-001-thumbnail.jpg");

#[derive(Deserialize)]
struct FixtureManifest {
    gallery_id: String,
    items: Vec<gallery::DemoPhoto>,
}

#[derive(Debug)]
pub enum RouterError {
    GalleryFixture(String),
    InvalidThumbnail(&'static str),
    InvalidOrigin,
    WebBundle(web_assets::BundleError),
}

impl std::fmt::Display for RouterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GalleryFixture(error) => write!(formatter, "invalid gallery fixture: {error}"),
            Self::InvalidThumbnail(error) => {
                write!(formatter, "invalid bundled thumbnail: {error}")
            }
            Self::InvalidOrigin => formatter.write_str("unable to construct local origin policy"),
            Self::WebBundle(error) => write!(formatter, "invalid web bundle: {error:?}"),
        }
    }
}

/// Build all route modules using the actual ephemeral listener authority.
/// Static files are loaded and validated before readiness is announced.
pub fn build_router(
    address: SocketAddr,
    web_root: Option<&Path>,
    auth: crate::http::session::runtime::SessionAuth,
) -> Result<Router, RouterError> {
    let manifest: FixtureManifest = serde_json::from_str(GALLERY_MANIFEST)
        .map_err(|error| RouterError::GalleryFixture(error.to_string()))?;
    let gallery = Arc::new(gallery::FixtureMap(BTreeMap::from([(
        manifest.gallery_id,
        manifest.items,
    )])));
    let thumbnail =
        thumbnail::ThumbnailAsset::from_declared_bundle(Arc::<[u8]>::from(THUMBNAIL_BYTES))
            .map_err(RouterError::InvalidThumbnail)?;

    let mut router = health::router(Arc::new(Ready))
        .merge(gallery::router(gallery))
        .merge(thumbnail::router(thumbnail));
    let mut public_assets = BTreeSet::new();
    if let Some(root) = web_root {
        let bundle =
            web_assets::AssetBundle::load_web_root(root).map_err(RouterError::WebBundle)?;
        public_assets = bundle.public_request_paths();
        router = bundle.router(router);
    }

    apply_origin_policy(router, address, auth, public_assets)
}

/// Build the persistent catalogue routes. The caller opens and initializes the
/// database before binding, so startup failures cannot be announced as ready.
pub fn build_catalog_router<C, R>(
    address: SocketAddr,
    web_root: Option<&Path>,
    catalog: C,
    recipes: R,
    analysis: Option<Router>,
    previews: Option<Router>,
    auth: crate::http::session::runtime::SessionAuth,
) -> Result<Router, RouterError>
where
    C: crate::http::catalog_reads::Catalog
        + crate::http::catalog_writes::decision::DecisionWriter
        + Clone
        + Send
        + Sync
        + 'static,
    R: crate::http::recipes::RecipeApi + 'static,
{
    let reads = crate::http::catalog_reads::routes::router::<C>()
        .with_state(Arc::new(crate::http::catalog_reads::Reads(catalog.clone())));
    let writes = crate::http::catalog_writes::decision::router::<C>().with_state(Arc::new(catalog));
    let recipes = crate::http::recipes::router::<R>().with_state(Arc::new(recipes));
    let mut router = health::router(Arc::new(Ready))
        .merge(reads)
        .merge(writes)
        .merge(recipes);
    if let Some(analysis) = analysis {
        router = router.merge(analysis);
    }
    if let Some(previews) = previews {
        router = router.merge(previews);
    }
    let mut public_assets = BTreeSet::new();
    if let Some(root) = web_root {
        let bundle =
            web_assets::AssetBundle::load_web_root(root).map_err(RouterError::WebBundle)?;
        public_assets = bundle.public_request_paths();
        router = bundle.router(router);
    }
    apply_origin_policy(router, address, auth, public_assets)
}

/// Build the persistent catalog router with server-local import endpoints.
/// Import source roots are resolved by `I`; no path is accepted from HTTP.
pub fn build_import_catalog_router<C, I, R>(
    address: SocketAddr,
    web_root: Option<&Path>,
    catalog: C,
    imports: I,
    recipes: R,
    analysis: Option<Router>,
    previews: Option<Router>,
    auth: crate::http::session::runtime::SessionAuth,
) -> Result<Router, RouterError>
where
    C: crate::http::catalog_reads::Catalog
        + crate::http::catalog_writes::decision::DecisionWriter
        + Clone
        + Send
        + Sync
        + 'static,
    I: crate::http::imports::routes::ImportApi + 'static,
    R: crate::http::recipes::RecipeApi + 'static,
{
    let reads = crate::http::catalog_reads::routes::router::<C>()
        .with_state(Arc::new(crate::http::catalog_reads::Reads(catalog.clone())));
    let writes = crate::http::catalog_writes::decision::router::<C>().with_state(Arc::new(catalog));
    let imports = crate::http::imports::routes::router::<I>().with_state(Arc::new(imports));
    let recipes = crate::http::recipes::router::<R>().with_state(Arc::new(recipes));
    let mut router = health::router(Arc::new(Ready))
        .merge(reads)
        .merge(writes)
        .merge(imports)
        .merge(recipes);
    if let Some(analysis) = analysis {
        router = router.merge(analysis);
    }
    if let Some(previews) = previews {
        router = router.merge(previews);
    }
    let mut public_assets = BTreeSet::new();
    if let Some(root) = web_root {
        let bundle =
            web_assets::AssetBundle::load_web_root(root).map_err(RouterError::WebBundle)?;
        public_assets = bundle.public_request_paths();
        router = bundle.router(router);
    }
    apply_origin_policy(router, address, auth, public_assets)
}

fn apply_origin_policy(
    router: Router,
    address: SocketAddr,
    auth: crate::http::session::runtime::SessionAuth,
    public_assets: BTreeSet<String>,
) -> Result<Router, RouterError> {
    let host = match address.ip() {
        std::net::IpAddr::V4(ip) => ip.to_string(),
        std::net::IpAddr::V6(ip) => format!("[{ip}]"),
    };
    let policy =
        OriginPolicy::new("http", &host, address.port()).map_err(|_| RouterError::InvalidOrigin)?;
    let router = router.merge(crate::http::session::runtime::router(auth.clone()));
    let router = crate::http::session::runtime::protect(router, auth, public_assets);
    Ok(crate::demo::ingress::protect(router, Arc::new(policy)))
}

struct Ready;
impl health::ReadinessProbe for Ready {
    fn ready(&self) -> bool {
        true
    }
}
