# D25 Firefox smoke

Run `npm install && npm test` from this directory. The Playwright run starts a
loopback fixture server and checks the browser version, same-origin API and
WASM requests, MIME types, visible JPEG dimensions, and accessible diagnostic
after simulated WASM initialization failure.

The generated-module fixture is not the Rust Slint application; output labels
`application_wasm: false`. D34 must rerun with the actual D20/D22/D23 bundle
served by Axum and record the Firefox version, requests, MIME types and visible
thumbnail result. A passing fixture smoke does not equal a real app acceptance.
