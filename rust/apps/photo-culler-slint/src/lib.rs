//! Shared Slint application and REST-backed demo client for desktop and WASM.

slint::include_modules!();

pub mod demo;
pub mod recipe_flow;
pub mod recipe_rest;

#[cfg(not(target_arch = "wasm32"))]
pub mod desktop;

#[cfg(not(target_arch = "wasm32"))]
pub mod file_selection;

#[cfg(any(target_arch = "wasm32", test))]
mod browser;
