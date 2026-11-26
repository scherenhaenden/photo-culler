//! Browser startup and same-origin REST endpoint rules.
//!
//! Browser-facing contract and WASM startup integration. The browser-only
//! transport and startup entry stay out of native builds.

#[cfg(any(target_arch = "wasm32", test))]
mod demo_fetch;

#[cfg(target_arch = "wasm32")]
mod demo_start;

pub const API_PREFIX: &str = "/api/v1";

#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(dead_code)] // Mirrored by the minimal JS bootstrap, covered here as a Rust contract.
pub enum StartupDiagnostic {
    WebGlUnavailable,
    WasmLoadFailed,
    AppStartFailed,
    ApiUnavailable,
}

#[allow(dead_code)] // The same recovery messages are rendered by the HTML bootstrap.
impl StartupDiagnostic {
    pub fn title(&self) -> &'static str {
        match self {
            Self::WebGlUnavailable => "Photo Culler cannot start in this browser",
            Self::WasmLoadFailed => "Photo Culler could not load its web application",
            Self::AppStartFailed => "Photo Culler could not open the interface",
            Self::ApiUnavailable => "Photo Culler cannot reach its local service",
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::WebGlUnavailable => {
                "WebGL is unavailable or disabled. Enable hardware acceleration or open Photo Culler Desktop."
            }
            Self::WasmLoadFailed => {
                "The WebAssembly files could not be loaded. Reload this page or open Photo Culler Desktop."
            }
            Self::AppStartFailed => {
                "The browser interface failed to initialize. Reload this page or open Photo Culler Desktop."
            }
            Self::ApiUnavailable => {
                "The local Photo Culler service did not respond. Restart the app or open Photo Culler Desktop."
            }
        }
    }
}

/// Build a relative same-origin API path. Reject absolute URLs and paths that
/// could escape the versioned API prefix; browser Fetch must never receive a
/// caller-supplied host or session token in the URL.
pub fn api_path(path: &str) -> Result<String, &'static str> {
    let suffix = path.trim_start_matches('/');
    if suffix.is_empty()
        || suffix.starts_with("api/v1/../")
        || suffix.split('/').any(|segment| segment == "..")
        || suffix.contains(['?', '#', '\\'])
        || suffix.contains("://")
    {
        return Err("invalid API path");
    }
    Ok(format!("{API_PREFIX}/{suffix}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_paths_are_relative_and_versioned() {
        assert_eq!(api_path("health"), Ok("/api/v1/health".into()));
        assert_eq!(api_path("/catalog?page=2"), Err("invalid API path"));
        assert_eq!(api_path("https://other.test/api"), Err("invalid API path"));
        assert_eq!(api_path("catalog/../admin"), Err("invalid API path"));
    }

    #[test]
    fn startup_diagnostics_include_a_native_recovery_path() {
        for diagnostic in [
            StartupDiagnostic::WebGlUnavailable,
            StartupDiagnostic::WasmLoadFailed,
            StartupDiagnostic::AppStartFailed,
            StartupDiagnostic::ApiUnavailable,
        ] {
            assert!(!diagnostic.title().is_empty());
            assert!(diagnostic.message().contains("Photo Culler Desktop"));
        }
        assert!(
            StartupDiagnostic::WebGlUnavailable
                .message()
                .contains("WebGL")
        );
    }
}
