// The browser target shares the Slint library but does not build the native
// desktop launcher. Keep a no-op binary entry so package-wide WASM checks work.
#[cfg(not(target_arch = "wasm32"))]
include!("../desktop_app.rs");

#[cfg(target_arch = "wasm32")]
fn main() {}
