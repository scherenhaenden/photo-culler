//! Axum demo server. The HTTP modules are assembled around the actual loopback
//! listener so its configured origin always contains the assigned port.

#[path = "http/runtime/lifecycle.rs"]
pub mod lifecycle;
#[path = "http/origin/mod.rs"]
pub mod origin;

pub mod demo {
    pub mod gallery;
    pub mod health;
    pub mod ingress;
    pub mod runtime;
    pub mod thumbnail;
    pub mod web_assets;
}

pub mod demo_args;
