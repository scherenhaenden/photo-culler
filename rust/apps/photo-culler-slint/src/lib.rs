//! Shared Slint package scaffold for the native and browser demo clients.

slint::include_modules!();

pub mod demo {
    pub mod flow;
    pub mod thumbnail_image;
}

#[cfg(not(target_arch = "wasm32"))]
pub mod desktop;

#[cfg(target_arch = "wasm32")]
mod browser;
