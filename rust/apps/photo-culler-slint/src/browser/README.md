# Browser startup

This area defines a minimal accessible startup page, a same-origin Fetch
transport for `/api/v1`, WebGL/WASM startup diagnostics and Rust unit tests for
the transport path and recovery messages. No credentials are placed in URLs.
The page checks the local health endpoint before loading the UI and presents a
clear recovery message when WebGL, the service, or the generated WASM app is
unavailable.

At sprint base `2caabb66`, the workspace has no `photo-culler-slint` crate,
Slint dependencies, browser build script, or generated wasm-bindgen adapter.
Accordingly the page expects the eventual generated `photo_culler_slint.js`
adapter to export `start(canvas, { fetchApi })` after its default WASM init.
The crate owner must register this module and adapter, build it for
`wasm32-unknown-unknown`, and validate in Chromium and Firefox. `fetchApi`
uses a relative `/api/v1/` URL, same-origin credentials, and rejects absolute,
traversal, query, and fragment inputs.

If WebGL is unavailable, the page explains how to enable hardware acceleration
and directs the user to open Photo Culler Desktop on the same computer.

Run the self-contained unit tests without Cargo:

```sh
rustc --edition=2024 --test rust/apps/photo-culler-slint/src/browser/mod.rs -o /tmp/photo-culler-task43-tests
/tmp/photo-culler-task43-tests
```
