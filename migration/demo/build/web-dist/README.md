# D23: same-origin web bundle staging

`stage.sh <generated-web-output> <staging-directory>` copies only the expected
HTML, bootstrap, wasm-bindgen JavaScript/WASM pair and optional regular files
under `assets/`. It writes `bundle.json` using the D09 contract:
`{"schema_version":1,"files":[relative names...]}`. `validate.sh` checks the
allowlist, required entries, path safety, file existence, duplicates, symlinks
and undeclared files before the directory can be published.

The server receives this one directory through
`photo-culler-server --port 0 --web-root <directory>` and serves the UI and
`/api/v1` from the assigned HTTP origin. The browser must not use a second
origin or enable CORS. D09 remains the owner of the Axum MIME map; the expected
types are `text/html` for HTML, `text/javascript` for generated/bootstrap JS,
`application/wasm` for WASM, and the declared media type for each generated
asset. The D09 acceptance should verify these headers over HTTP.

Run `migration/demo/build/web-dist/test.sh` for fixture staging, a missing
required file, an out-of-root path, and an undeclared file. These are bundle
validator tests, not a browser run. A complete package build still requires
D11, D20 and D22's generated files; real same-origin MIME and rendering checks
belong to D24/D25 and the full D34 acceptance.
