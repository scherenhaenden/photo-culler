//! Host/Origin policy independent of the future HTTP framework.
//! See README.md for the required adapter and integration boundaries.
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

/// Safe initial bind: loopback, with the operating system selecting the port.
pub fn default_bind_address() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 0)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Rejection {
    InvalidConfiguration,
    InvalidHost,
    InvalidOrigin,
    CrossSite,
    InvalidMethod,
}

/// An explicitly configured canonical HTTP(S) origin, including its actual port.
/// Never derive this value from request headers.
#[derive(Debug)]
pub struct OriginPolicy {
    authority: String,
    origin: String,
}

impl OriginPolicy {
    pub fn new(scheme: &str, host: &str, port: u16) -> Result<Self, Rejection> {
        if !matches!(scheme, "http" | "https") || port == 0 || !valid_host(host) {
            return Err(Rejection::InvalidConfiguration);
        }
        let host = host.to_ascii_lowercase();
        let authority = format!("{host}:{port}");
        Ok(Self {
            origin: format!("{scheme}://{authority}"),
            authority,
        })
    }

    /// Validate *all* values of each header, before routing or authentication.
    /// Browser mutations require an exact Origin; native clients may omit Origin
    /// only on the separately authenticated native-client path described in README.
    pub fn check(
        &self,
        method: &str,
        hosts: &[&str],
        origins: &[&str],
        fetch_sites: &[&str],
        native_authenticated: bool,
    ) -> Result<(), Rejection> {
        if method.is_empty() || !method.bytes().all(|b| b.is_ascii_uppercase() || b == b'-') {
            return Err(Rejection::InvalidMethod);
        }
        if hosts.len() != 1 || !hosts[0].eq_ignore_ascii_case(&self.authority) {
            return Err(Rejection::InvalidHost);
        }
        if fetch_sites.len() > 1
            || fetch_sites
                .iter()
                .any(|v| !matches!(*v, "same-origin" | "none"))
        {
            return Err(Rejection::CrossSite);
        }
        if origins.len() > 1 || origins.iter().any(|v| *v != self.origin) {
            return Err(Rejection::InvalidOrigin);
        }
        let safe = matches!(method, "GET" | "HEAD" | "OPTIONS");
        if !safe && origins.is_empty() && !native_authenticated {
            return Err(Rejection::InvalidOrigin);
        }
        Ok(())
    }
}

fn valid_host(host: &str) -> bool {
    if host.starts_with('[') && host.ends_with(']') {
        return host[1..host.len() - 1]
            .parse::<std::net::Ipv6Addr>()
            .is_ok();
    }
    if host.is_empty() || host.len() > 253 || !host.is_ascii() {
        return false;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && label.as_bytes()[0].is_ascii_alphanumeric()
            && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

/// Apply to successful frontend assets, replacing any conflicting header values.
/// CSP permits same-origin WASM compilation without enabling arbitrary script eval.
pub fn frontend_headers(is_wasm: bool) -> &'static [(&'static str, &'static str)] {
    if is_wasm {
        WASM_HEADERS
    } else {
        HTML_HEADERS
    }
}
const HTML_HEADERS: &[(&str, &str)] = &[
    ("content-type", "text/html; charset=utf-8"),
    ("x-content-type-options", "nosniff"),
    ("content-security-policy", "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'"),
    ("referrer-policy", "no-referrer"),
];
const WASM_HEADERS: &[(&str, &str)] = &[
    ("content-type", "application/wasm"),
    ("x-content-type-options", "nosniff"),
    ("content-security-policy", "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'"),
    ("referrer-policy", "no-referrer"),
];

#[cfg(test)]
mod tests {
    use super::*;
    struct FakeRouter {
        policy: OriginPolicy,
    }
    impl FakeRouter {
        fn request(
            &self,
            method: &str,
            path: &str,
            host: &[&str],
            origin: &[&str],
            site: &[&str],
        ) -> (u16, &'static [(&'static str, &'static str)]) {
            if self
                .policy
                .check(method, host, origin, site, false)
                .is_err()
            {
                return (403, &[]);
            }
            match path {
                "/" => (200, frontend_headers(false)),
                "/app.wasm" => (200, frontend_headers(true)),
                "/api/v1/decision" => (204, &[]),
                _ => (404, &[]),
            }
        }
    }
    fn router() -> FakeRouter {
        FakeRouter {
            policy: OriginPolicy::new("http", "127.0.0.1", 32100).unwrap(),
        }
    }
    #[test]
    fn acceptance_browser_frontend_and_foreign_write() {
        assert_eq!(default_bind_address(), "127.0.0.1:0".parse().unwrap());
        let r = router();
        let host = ["127.0.0.1:32100"];
        let origin = ["http://127.0.0.1:32100"];
        let (status, html) = r.request("GET", "/", &host, &[], &["none"]);
        assert_eq!(status, 200);
        assert!(html.contains(&("content-type", "text/html; charset=utf-8")));
        let (status, wasm) = r.request("GET", "/app.wasm", &host, &origin, &["same-origin"]);
        assert_eq!(status, 200);
        assert!(wasm.contains(&("content-type", "application/wasm")));
        assert!(wasm.contains(&("x-content-type-options", "nosniff")));
        assert_eq!(
            r.request("POST", "/api/v1/decision", &host, &origin, &["same-origin"])
                .0,
            204
        );
        assert_eq!(
            r.request(
                "POST",
                "/api/v1/decision",
                &host,
                &["http://evil.example"],
                &["cross-site"]
            )
            .0,
            403
        );
    }
    #[test]
    fn rejects_ambiguous_or_forged_headers_and_missing_write_origin() {
        let p = &router().policy;
        for hosts in [
            &[][..],
            &["evil.example:32100"][..],
            &["127.0.0.1:32100", "127.0.0.1:32100"][..],
        ] {
            assert_eq!(
                p.check("GET", hosts, &[], &[], false),
                Err(Rejection::InvalidHost)
            );
        }
        let h = &["127.0.0.1:32100"];
        for origins in [
            &[][..],
            &["null"][..],
            &["http://127.0.0.1:32100.evil"][..],
            &["http://127.0.0.1:32100", "http://127.0.0.1:32100"][..],
        ] {
            assert_eq!(
                p.check("POST", h, origins, &[], false),
                Err(Rejection::InvalidOrigin)
            );
        }
        assert_eq!(
            p.check("GET", h, &[], &["same-site"], false),
            Err(Rejection::CrossSite)
        );
        assert_eq!(
            p.check("GET", h, &[], &["same-origin", "same-origin"], false),
            Err(Rejection::CrossSite)
        );
        assert!(p.check("POST", h, &[], &[], true).is_ok());
        assert!(p.check("POST", h, &["http://evil"], &[], true).is_err());
        assert!(p.check("post", h, &[], &[], true).is_err());
    }
    #[test]
    fn configuration_is_explicit_and_ipv6_supported() {
        for host in [
            "",
            "localhost:123",
            "localhost.evil/",
            "user@localhost",
            "localhost.",
            "[::1]:3",
        ] {
            assert_eq!(
                OriginPolicy::new("http", host, 5).unwrap_err(),
                Rejection::InvalidConfiguration
            );
        }
        assert!(OriginPolicy::new("http", "localhost", 0).is_err());
        let p = OriginPolicy::new("http", "[::1]", 12).unwrap();
        assert!(p
            .check("POST", &["[::1]:12"], &["http://[::1]:12"], &[], false)
            .is_ok());
    }
}
