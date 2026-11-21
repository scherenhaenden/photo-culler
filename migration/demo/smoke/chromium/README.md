# D24 Chromium live Rust/Slint smoke

Run `npm install && npm test` from this directory. The Playwright run starts
the built Rust Axum server with the generated Slint bundle, then loads the
actual application. It checks same-origin health, gallery, thumbnail and WASM
responses, waits until Rust has decoded the JPEG and handed it to Slint, and
captures the rendered screen. CI supplies `PHOTO_CULLER_SERVER` and
`PHOTO_CULLER_WEB_ROOT` from build artifacts.
