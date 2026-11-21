# Browser startup

This area defines a minimal startup page, a same-origin Fetch transport for
`/api/v1`, and WebGL/WASM startup diagnostics. The Rust WASM entry opens the
shared `DemoWindow`, fetches health, the gallery and its JPEG, then updates the
visible screen. No credentials are placed in URLs.

The generated `photo_culler_slint.js` adapter starts the WASM entry after its
default initialization. Fetch uses relative `/api/v1/` paths, same-origin
credentials, and rejects absolute, traversal, query, and fragment inputs.

If WebGL is unavailable, the page explains how to enable hardware acceleration
and directs the user to open Photo Culler Desktop on the same computer.

Run the self-contained unit tests without Cargo:

```sh
rustc --edition=2024 --test rust/apps/photo-culler-slint/src/browser/mod.rs -o /tmp/photo-culler-task43-tests
/tmp/photo-culler-task43-tests
```
