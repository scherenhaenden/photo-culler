# D24 Chromium smoke

Run `npm install && npm test` from this directory. The Playwright run starts a
loopback fixture server, checks the WASM MIME, health/gallery/thumbnail HTTP
responses, same-origin requests, visible JPEG dimensions, and the accessible
startup diagnostic on a simulated WASM initialization failure. It prints the
Chromium version, request list and result as JSON.

This fixture emulates a generated WASM module and is a browser/HTTP smoke, not
proof that the Rust Slint application compiled or ran. It intentionally labels
`application_wasm: false`. D34 must repeat the smoke with D20/D22/D23 artifacts
served by Axum from one origin and record the real thumbnail result.
