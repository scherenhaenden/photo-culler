//! Shared Slint package scaffold for the native and browser demo clients.

slint::include_modules!();

#[cfg(target_arch = "wasm32")]
mod browser;
