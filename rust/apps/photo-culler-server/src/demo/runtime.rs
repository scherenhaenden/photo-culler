//! Executable demo router assembled from the checked-in API and fixture contracts.

use std::{collections::BTreeMap, net::SocketAddr, path::Path, sync::Arc};

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
pub fn build_router(address: SocketAddr, web_root: Option<&Path>) -> Result<Router, RouterError> {
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
    if let Some(root) = web_root {
        router = web_assets::AssetBundle::load_web_root(root)
            .map_err(RouterError::WebBundle)?
            .router(router);
    }

    let host = match address.ip() {
        std::net::IpAddr::V4(ip) => ip.to_string(),
        std::net::IpAddr::V6(ip) => format!("[{ip}]"),
    };
    let policy =
        OriginPolicy::new("http", &host, address.port()).map_err(|_| RouterError::InvalidOrigin)?;
    Ok(crate::demo::ingress::protect(router, Arc::new(policy)))
}

struct Ready;
impl health::ReadinessProbe for Ready {
    fn ready(&self) -> bool {
        true
    }
}
