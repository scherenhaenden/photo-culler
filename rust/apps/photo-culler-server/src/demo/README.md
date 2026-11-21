# Read-only fixture server

The server now starts an HTTP listener on loopback and serves the declared
synthetic `demo-001` gallery, its bundled JPEG thumbnail, and the readiness
route. It accepts an optional generated Slint web bundle through `--static-dir`.
The bundle is validated and loaded before the readiness line is written.

Build both executables so the desktop client can find its sibling server, then
open the desktop gallery:

```sh
cargo build --offline --locked --manifest-path rust/Cargo.toml \
  -p photo-culler-server -p photo-culler-slint
rust/target/debug/photo-culler-desktop
```

The desktop process starts its sibling server on an assigned loopback port,
loads health, gallery JSON and the thumbnail in a worker thread, and displays
the JPEG. Readiness has a 10 second deadline; retries return to loading and
discard results from earlier requests. Closing the window closes the server's
piped stdin and waits for graceful shutdown. To run the server independently, use
`cargo run --offline --manifest-path rust/Cargo.toml -p photo-culler-server --
--port 0`; its first stdout line is a JSON `ready` message with the assigned
origin, and `Ctrl-C` performs graceful shutdown.

`--data-dir` is deliberately rejected: this fixture server is read-only and
does not persist or import a catalog. Non-loopback binding is rejected by
argument parsing and again by the HTTP lifecycle.

Routes:

- `GET /api/v1/health`
- `GET /api/v1/galleries/demo/photos`
- `GET /api/v1/photos/demo-001/thumbnail/800`
- With `--static-dir`: `/` and the four explicitly declared HTML/JS/WASM files.

Every response uses `Cache-Control: no-store`. HTML and WASM receive the
configured CSP, `nosniff` and `no-referrer` headers; JavaScript receives
`nosniff` and its declared JavaScript MIME type. Asset requests are restricted
to the declared bundle list.

The desktop and browser Slint windows now consume this fixture response. This
is still a synthetic, one-photo demo flow; it is not acceptance of a real user
catalog, pagination, RAW support, or the full product workflow.

## Recorded verification

On 2026-11-21, offline locked `cargo check` passed for the server and Slint
packages on native macOS and for Slint on `wasm32-unknown-unknown`. The 25
server unit tests passed, including loopback lifecycle and asset header checks.
A native build of both executables also passed. A local loopback smoke confirmed
`ready`, health `ready`, the `demo-001` gallery row, and the JPEG response
(8,304 bytes; SHA-256
`cc20c24844d2fc6fca729fcb66cfe661960f7b56be41a3dd462a0f1b22cfb4f2`). The
smoke process closed its stdin and exited before cleanup. Visual GUI acceptance
and loading a generated WASM bundle through the integrated server remain open.
