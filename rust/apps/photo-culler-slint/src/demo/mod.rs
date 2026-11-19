//! Shared gallery transport adapter and Slint view binding.

pub mod controller;
pub mod rest;
pub mod slint_view;
pub mod thumbnail_image;

#[cfg(not(target_arch = "wasm32"))]
pub mod native_transport;
