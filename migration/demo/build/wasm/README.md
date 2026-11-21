# D20: WASM toolchain

Run `./migration/demo/build/wasm/build.sh` from any working directory. It
checks that `wasm32-unknown-unknown` is installed, compares the `wasm-bindgen`
CLI version with the package version pinned in `rust/Cargo.lock`, builds the
D11 package with `cargo build --locked`, then runs `wasm-bindgen --target web`.
The generated entry consumed by D23 is
`migration/demo/build/web-dist/photo_culler_slint.js` and its sibling
`photo_culler_slint_bg.wasm`.

Install prerequisites with `rustup target add wasm32-unknown-unknown` and
`cargo install wasm-bindgen-cli --version <version-from-Cargo.lock>`. The
target and CLI checks fail early with the exact corrective command. This
recipe does not add native windowing dependencies to the WASM build; D11 must
keep platform-specific Slint features behind target configuration.

The checkout at the D20 base does not yet contain D11's `Cargo.toml`, so a
real package build remains pending until D11 is integrated. The local runner
also currently lacks the WASM target and `wasm-bindgen` CLI. Script checks can
still be tested with fake tool executables; those checks do not count as a
WASM compilation. Compilation and browser execution are separate evidence:
Chromium and Firefox runs belong to D24/D25 and must report their own versions,
requests, MIME types and visible thumbnail result.
