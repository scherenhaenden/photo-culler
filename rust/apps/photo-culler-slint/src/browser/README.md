# Browser startup

This area defines a minimal accessible startup page, a same-origin Fetch
transport for `/api/v1`, WebGL/WASM startup diagnostics and Rust unit tests for
the transport path and recovery messages. No credentials are placed in URLs.
The page checks the local health endpoint before loading the UI and presents a
clear recovery message when WebGL, the service, or the generated WASM app is
unavailable.

Historical note for sprint base `2caabb66`: that snapshot had no
`photo-culler-slint` crate, Slint dependencies, browser build script, or
generated wasm-bindgen adapter. Those pieces have since been added. The current
browser entry point and shared import dialog are wired in
`browser/demo_start.rs`; its same-origin transport uses relative API paths.
The accepted browser evidence still covers the S01 fixture flow. The candidate
REST v2 import path has not yet received runtime acceptance; follow the manual
procedure in `docs/rust-slint-demo/RUN.md` and record results against its exact
build SHA before claiming it works in a browser.

If WebGL is unavailable, the page explains how to enable hardware acceleration
and directs the user to open Photo Culler Desktop on the same computer.

The standalone Task 43 transport checks below are historical unit checks. They
do not exercise the current workspace build, the Rust server, or REST v2 import
at runtime, and are not browser acceptance evidence:

```sh
rustc --edition=2024 --test rust/apps/photo-culler-slint/src/browser/mod.rs -o /tmp/photo-culler-task43-tests
/tmp/photo-culler-task43-tests
```

The current candidate adds a same-origin pairing overlay. The browser keeps its
bearer in WASM memory only, sends it in `Authorization` with Fetch credentials
omitted, and returns to pairing after a 401. Reloading loses the token. This
path compiles for WASM but has no current browser-window acceptance; a
non-interactive/service server also has no initial-code handoff yet.
