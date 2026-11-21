//! Integrated loopback router for the bundled, read-only gallery demo.

use std::{collections::BTreeMap, io, net::SocketAddr, path::Path, sync::Arc};

use axum::Router;
use tokio::io::AsyncReadExt;

use crate::{
    demo::{gallery, health, ingress, thumbnail, web_assets},
    demo_args::DemoArgs,
    http::runtime::lifecycle,
    origin::OriginPolicy,
};

const FIXTURE_THUMBNAIL: &[u8] =
    include_bytes!("../../../../../migration/demo/fixture/demo-001-thumbnail.jpg");

struct Ready;

impl health::ReadinessProbe for Ready {
    fn ready(&self) -> bool {
        true
    }
}

pub fn build_router(address: SocketAddr, static_dir: Option<&Path>) -> Result<Router, String> {
    if !address.ip().is_loopback() || address.port() == 0 {
        return Err("the demo server requires an assigned loopback address".into());
    }

    let photo = gallery::DemoPhoto {
        photo_id: "demo-001".into(),
        filename: "demo-001.jpg".into(),
        decision: "undecided".into(),
        representation: "jpeg".into(),
        thumbnail_url: "/api/v1/photos/demo-001/thumbnail/800".into(),
    };
    let fixture = Arc::new(gallery::FixtureMap(BTreeMap::from([(
        "demo".into(),
        vec![photo],
    )])));
    let thumbnail =
        thumbnail::ThumbnailAsset::from_declared_bundle(Arc::<[u8]>::from(FIXTURE_THUMBNAIL))
            .map_err(str::to_owned)?;

    let host = match address.ip() {
        std::net::IpAddr::V4(ip) => ip.to_string(),
        std::net::IpAddr::V6(ip) => format!("[{ip}]"),
    };
    let origin = Arc::new(
        OriginPolicy::new("http", &host, address.port())
            .map_err(|_| "could not configure the loopback origin policy")?,
    );
    let api = Router::new()
        .merge(health::router(Arc::new(Ready)))
        .merge(gallery::router(fixture))
        .merge(thumbnail::router(thumbnail));

    let app = match static_dir {
        Some(root) => {
            let bundle = web_assets::AssetBundle::load(root, &web_assets::standard_declarations())
                .map_err(|error| format!("could not load web bundle: {error:?}"))?;
            bundle.router(api)
        }
        None => api,
    };
    Ok(ingress::protect(app, origin))
}

pub async fn run(args: DemoArgs) -> Result<(), String> {
    if args.data_dir.is_some() {
        return Err("--data-dir is not supported by the read-only fixture server".into());
    }
    let address = SocketAddr::new(args.bind, args.port);
    let static_dir = args.static_dir;
    let mut stdout = io::stdout().lock();
    let server = lifecycle::start_and_announce_on(
        address,
        move |actual_address| build_router(actual_address, static_dir.as_deref()),
        &mut stdout,
    )
    .await
    .map_err(|error| error.to_string())?;
    drop(stdout);

    let stdin_eof = async {
        let mut stdin = tokio::io::stdin();
        let mut byte = [0_u8; 1];
        loop {
            match stdin.read(&mut byte).await {
                Ok(0) | Err(_) => break,
                Ok(_) => {}
            }
        }
    };
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    server
        .wait_for_shutdown(stdin_eof, interrupt)
        .await
        .map_err(|error| format!("server shutdown failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };
    use tower::ServiceExt;

    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

    struct TempBundle(PathBuf);

    impl TempBundle {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "pc-demo-runtime-{}-{}",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            for (name, bytes) in [
                ("index.html", b"<html>demo</html>".as_slice()),
                ("bootstrap.js", b"boot".as_slice()),
                ("photo_culler_slint.js", b"js".as_slice()),
                ("photo_culler_slint_bg.wasm", b"\0asm\x01\0\0\0".as_slice()),
            ] {
                fs::write(path.join(name), bytes).unwrap();
            }
            Self(path)
        }
    }

    impl Drop for TempBundle {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[tokio::test]
    async fn integrated_router_serves_assets_and_api_with_security_and_no_store_headers() {
        let bundle = TempBundle::new();
        let address = SocketAddr::from(([127, 0, 0, 1], 49_152));
        let app = build_router(address, Some(&bundle.0)).unwrap();

        let page = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("host", "127.0.0.1:49152")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(page.status(), axum::http::StatusCode::OK);
        assert_eq!(page.headers()["content-type"], "text/html; charset=utf-8");
        assert_eq!(page.headers()["cache-control"], "no-store");
        assert_eq!(page.headers()["x-content-type-options"], "nosniff");
        assert!(page.headers().contains_key("content-security-policy"));
        assert_eq!(page.headers()["referrer-policy"], "no-referrer");

        let api = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/galleries/demo/photos")
                    .header("host", "127.0.0.1:49152")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(api.status(), axum::http::StatusCode::OK);
        assert_eq!(api.headers()["cache-control"], "no-store");
    }
}
