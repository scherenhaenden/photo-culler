//! Axum serving for an explicit, prevalidated web bundle. No filesystem paths
//! are derived from request URLs.
//! Contract source: issues #83/#84 and architecture contract in docs/rust-slint-demo;
//! demo-specific docs are local/untracked and are not copied. Sprint base: db9a007.

use axum::{
    Router,
    body::Body,
    extract::State,
    http::{HeaderValue, Response, StatusCode, Uri, header::CONTENT_TYPE},
    routing::get,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Component, Path},
    sync::Arc,
};

#[derive(Clone, Debug)]
pub struct WebAsset {
    pub relative_name: String,
}

#[derive(Clone, Debug)]
pub struct AssetBundle {
    files: Arc<BTreeMap<String, (Vec<u8>, &'static str)>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BundleError {
    InvalidRoot,
    InvalidManifest,
    UnsupportedManifestVersion,
    InvalidName,
    DuplicateName,
    UnsupportedType,
    MissingRequired(String),
    OutsideRoot,
    SymlinkNotAllowed,
    UndeclaredFile(String),
    ReadFailed,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BundleManifest {
    schema_version: u32,
    files: Vec<String>,
}

impl AssetBundle {
    /// Read and validate the D23 `bundle.json` allowlist, then preload only its
    /// declared regular files. The manifest itself is never served.
    pub fn load_web_root(root: impl AsRef<Path>) -> Result<Self, BundleError> {
        const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
        let root = fs::canonicalize(root).map_err(|_| BundleError::InvalidRoot)?;
        if !root.is_dir() {
            return Err(BundleError::InvalidRoot);
        }
        let manifest_path = root.join("bundle.json");
        let metadata =
            fs::symlink_metadata(&manifest_path).map_err(|_| BundleError::InvalidManifest)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_MANIFEST_BYTES
        {
            return Err(BundleError::InvalidManifest);
        }
        let manifest_bytes = fs::read(&manifest_path).map_err(|_| BundleError::ReadFailed)?;
        let manifest: BundleManifest =
            serde_json::from_slice(&manifest_bytes).map_err(|_| BundleError::InvalidManifest)?;
        if manifest.schema_version != 1 {
            return Err(BundleError::UnsupportedManifestVersion);
        }
        if manifest.files.is_empty() || manifest.files.iter().any(|file| file == "bundle.json") {
            return Err(BundleError::InvalidManifest);
        }
        let declarations = manifest
            .files
            .into_iter()
            .map(|relative_name| WebAsset { relative_name })
            .collect::<Vec<_>>();
        let bundle = Self::load(&root, &declarations)?;

        let mut actual = BTreeSet::new();
        collect_regular_files(&root, &root, &mut actual)?;
        actual.remove("bundle.json");
        for file in actual {
            if !bundle.files.contains_key(&file) {
                return Err(BundleError::UndeclaredFile(file));
            }
        }
        Ok(bundle)
    }

    /// Validate and preload only names explicitly declared by the build manifest.
    pub fn load(root: impl AsRef<Path>, declarations: &[WebAsset]) -> Result<Self, BundleError> {
        let root = fs::canonicalize(root).map_err(|_| BundleError::InvalidRoot)?;
        if !root.is_dir() {
            return Err(BundleError::InvalidRoot);
        }
        let mut files = BTreeMap::new();
        for declaration in declarations {
            let relative = Path::new(&declaration.relative_name);
            if relative.as_os_str().is_empty()
                || relative.is_absolute()
                || !declaration.relative_name.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b'/')
                })
                || declaration
                    .relative_name
                    .split('/')
                    .any(|segment| segment.is_empty() || segment == "." || segment == "..")
                || relative
                    .components()
                    .any(|c| !matches!(c, Component::Normal(_)))
            {
                return Err(BundleError::InvalidName);
            }
            let candidate = root.join(relative);
            let canonical = fs::canonicalize(&candidate).map_err(|_| BundleError::ReadFailed)?;
            if !canonical.starts_with(&root) {
                return Err(BundleError::OutsideRoot);
            }
            let mime =
                content_type(&declaration.relative_name).ok_or(BundleError::UnsupportedType)?;
            let bytes = fs::read(canonical).map_err(|_| BundleError::ReadFailed)?;
            if files
                .insert(declaration.relative_name.clone(), (bytes, mime))
                .is_some()
            {
                return Err(BundleError::DuplicateName);
            }
        }
        for required in [
            "index.html",
            "bootstrap.js",
            "photo_culler_slint.js",
            "photo_culler_slint_bg.wasm",
        ] {
            if !files.contains_key(required) {
                return Err(BundleError::MissingRequired(required.to_owned()));
            }
        }
        Ok(Self {
            files: Arc::new(files),
        })
    }

    pub fn router(self, api: Router) -> Router {
        let assets = Router::new()
            .route("/", get(root_asset))
            .fallback(serve_declared_asset)
            .with_state(self);
        api.merge(assets)
    }
}

fn content_type(name: &str) -> Option<&'static str> {
    match Path::new(name)
        .extension()?
        .to_str()?
        .to_ascii_lowercase()
        .as_str()
    {
        "html" => Some("text/html; charset=utf-8"),
        "js" => Some("text/javascript; charset=utf-8"),
        "wasm" => Some("application/wasm"),
        "css" => Some("text/css; charset=utf-8"),
        "svg" => Some("image/svg+xml"),
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "webp" => Some("image/webp"),
        "ico" => Some("image/x-icon"),
        "json" => Some("application/json"),
        _ => None,
    }
}

fn collect_regular_files(
    root: &Path,
    directory: &Path,
    output: &mut BTreeSet<String>,
) -> Result<(), BundleError> {
    for entry in fs::read_dir(directory).map_err(|_| BundleError::ReadFailed)? {
        let entry = entry.map_err(|_| BundleError::ReadFailed)?;
        let file_type = entry.file_type().map_err(|_| BundleError::ReadFailed)?;
        if file_type.is_symlink() {
            return Err(BundleError::SymlinkNotAllowed);
        }
        if file_type.is_dir() {
            collect_regular_files(root, &entry.path(), output)?;
        } else if file_type.is_file() {
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|_| BundleError::OutsideRoot)?
                .to_str()
                .ok_or(BundleError::InvalidName)?
                .replace(std::path::MAIN_SEPARATOR, "/");
            output.insert(relative);
        } else {
            return Err(BundleError::InvalidName);
        }
    }
    Ok(())
}

async fn root_asset(State(bundle): State<AssetBundle>) -> Response<Body> {
    serve_name(bundle, "index.html")
}
async fn serve_declared_asset(State(bundle): State<AssetBundle>, uri: Uri) -> Response<Body> {
    let Some(path) = uri.path().strip_prefix('/') else {
        return not_found();
    };
    serve_name(bundle, path)
}

fn serve_name(bundle: AssetBundle, name: &str) -> Response<Body> {
    let Some((bytes, mime)) = bundle.files.get(name) else {
        return not_found();
    };
    let mut response = Response::new(Body::from(bytes.clone()));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(mime));
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
}
fn not_found() -> Response<Body> {
    let mut response = Response::new(Body::from(
        r#"{"code":"not_found","message":"Resource was not found","request_id":"req-example"}"#,
    ));
    *response.status_mut() = StatusCode::NOT_FOUND;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
}

pub fn standard_declarations() -> [WebAsset; 4] {
    [
        WebAsset {
            relative_name: "index.html".into(),
        },
        WebAsset {
            relative_name: "bootstrap.js".into(),
        },
        WebAsset {
            relative_name: "photo_culler_slint.js".into(),
        },
        WebAsset {
            relative_name: "photo_culler_slint_bg.wasm".into(),
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::to_bytes, http::Request};
    use std::{
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };
    use tower::ServiceExt;
    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Temp(PathBuf);
    impl Temp {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "pc-d09-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture() -> Temp {
        let temp = Temp::new();
        for (name, bytes) in [
            ("index.html", b"<html>demo</html>".as_slice()),
            ("bootstrap.js", b"boot".as_slice()),
            ("photo_culler_slint.js", b"js".as_slice()),
            ("photo_culler_slint_bg.wasm", b"\0asm\x01\0\0\0".as_slice()),
            ("secret.txt", b"secret".as_slice()),
        ] {
            fs::write(temp.0.join(name), bytes).unwrap();
        }
        temp
    }

    fn web_fixture() -> Temp {
        let temp = fixture();
        fs::remove_file(temp.0.join("secret.txt")).unwrap();
        fs::create_dir(temp.0.join("assets")).unwrap();
        fs::write(temp.0.join("assets/icon.svg"), b"<svg/>").unwrap();
        let manifest = serde_json::json!({
            "schema_version": 1,
            "files": [
                "index.html", "bootstrap.js", "photo_culler_slint.js",
                "photo_culler_slint_bg.wasm", "assets/icon.svg"
            ]
        });
        fs::write(
            temp.0.join("bundle.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        temp
    }

    #[test]
    fn manifest_loads_declared_nested_assets_and_mime_types() {
        let temp = web_fixture();
        let bundle = AssetBundle::load_web_root(&temp.0).unwrap();
        let response = serve_name(bundle, "assets/icon.svg");
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_TYPE], "image/svg+xml");
    }

    #[test]
    fn manifest_rejects_version_path_symlink_and_undeclared_file() {
        let temp = web_fixture();
        fs::write(temp.0.join("extra.js"), b"not declared").unwrap();
        assert_eq!(
            AssetBundle::load_web_root(&temp.0).unwrap_err(),
            BundleError::UndeclaredFile("extra.js".into())
        );
        fs::remove_file(temp.0.join("extra.js")).unwrap();
        let bad = serde_json::json!({"schema_version": 2, "files": ["index.html"]});
        fs::write(
            temp.0.join("bundle.json"),
            serde_json::to_vec(&bad).unwrap(),
        )
        .unwrap();
        assert_eq!(
            AssetBundle::load_web_root(&temp.0).unwrap_err(),
            BundleError::UnsupportedManifestVersion
        );
        let bad = serde_json::json!({"schema_version": 1, "files": ["../outside.js"]});
        fs::write(
            temp.0.join("bundle.json"),
            serde_json::to_vec(&bad).unwrap(),
        )
        .unwrap();
        assert_eq!(
            AssetBundle::load_web_root(&temp.0).unwrap_err(),
            BundleError::InvalidName
        );
    }
    #[tokio::test]
    async fn declared_html_js_and_wasm_have_expected_mimes() {
        let temp = fixture();
        let bundle = AssetBundle::load(&temp.0, &standard_declarations()).unwrap();
        let app = bundle.router(Router::new());
        for (uri, mime, bytes) in [
            (
                "/",
                "text/html; charset=utf-8",
                b"<html>demo</html>".as_slice(),
            ),
            (
                "/bootstrap.js",
                "text/javascript; charset=utf-8",
                b"boot".as_slice(),
            ),
            (
                "/photo_culler_slint_bg.wasm",
                "application/wasm",
                b"\0asm\x01\0\0\0".as_slice(),
            ),
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[CONTENT_TYPE], mime);
            assert_eq!(
                to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
                bytes
            );
        }
    }
    #[tokio::test]
    async fn undeclared_and_api_paths_are_real_404s_not_index_fallback() {
        let temp = fixture();
        let app = AssetBundle::load(&temp.0, &standard_declarations())
            .unwrap()
            .router(Router::new());
        for uri in ["/secret.txt", "/missing.js", "/api/v1/unknown"] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }
    }
    #[test]
    fn invalid_roots_names_missing_files_and_symlink_escape_fail_before_start() {
        assert_eq!(
            AssetBundle::load("/definitely/not/a/real/root", &standard_declarations()).unwrap_err(),
            BundleError::InvalidRoot
        );
        let temp = fixture();
        let mut bad = standard_declarations();
        bad[0].relative_name = "../secret.txt".into();
        assert_eq!(
            AssetBundle::load(&temp.0, &bad).unwrap_err(),
            BundleError::InvalidName
        );
        let mut missing = standard_declarations();
        missing[3].relative_name = "missing.wasm".into();
        assert_eq!(
            AssetBundle::load(&temp.0, &missing).unwrap_err(),
            BundleError::ReadFailed
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let outside = std::env::temp_dir().join(format!(
                "pc-d09-outside-{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::write(&outside, b"outside").unwrap();
            symlink(&outside, temp.0.join("escape.js")).unwrap();
            let decl = [WebAsset {
                relative_name: "escape.js".into(),
            }];
            assert_eq!(
                AssetBundle::load(&temp.0, &decl).unwrap_err(),
                BundleError::OutsideRoot
            );
            let _ = fs::remove_file(outside);
        }
    }
}
