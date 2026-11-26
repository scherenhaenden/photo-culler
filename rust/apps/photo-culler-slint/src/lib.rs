//! Shared Slint application and REST-backed demo client for desktop and WASM.

slint::include_modules!();

pub mod demo;
pub mod recipe_flow;
pub mod recipe_rest;

#[path = "../ui/inspector/mod.rs"]
pub mod inspector;

#[cfg(not(target_arch = "wasm32"))]
pub mod desktop;

#[cfg(not(target_arch = "wasm32"))]
pub mod file_selection;

#[cfg(target_arch = "wasm32")]
mod browser;
