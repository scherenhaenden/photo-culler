# Rust/Slint browser UI-state smoke

This directory provides deterministic visual acceptance for the shared `DemoWindow`
state panel. It loads the built Slint WebAssembly app and its JavaScript glue from
the real Rust Axum loopback origin. Playwright intercepts only the gallery response
for the `empty` and first `error`/`retry` responses; health, application assets, and
the successful retry gallery plus JPEG are served by Axum.

The run covers these states at 1440×900 and 1100×700 CSS pixels, device scale 1:

- `loading`: hold the actual gallery request, capture the rendered loading panel,
  then release it to the real Rust API and confirm the page reaches `ready`.
- `empty`: return the canonical empty gallery DTO and capture the rendered empty
  message while the Rust health endpoint and actual WASM application remain active.
- `error`: return a typed HTTP 503 API error and capture the rendered error state.
- `retry-loading` → `retry-ready`: activate the visible Slint retry control, hold
  the second gallery request long enough to capture loading, then continue to the
  real Rust gallery/JPEG routes and verify recovery.

The page's `#application-status` live region mirrors the state and provides a
stable DOM assertion. Each screenshot also requires a visible Slint canvas and
records its CSS/backing dimensions, viewport, state transition history, fixture
responses, source SHA, browser version, and PNG SHA-256 in `manifest.json`.
The harness decodes each PNG and requires rendered, non-dark pixels in the
gallery region, so a blank canvas/state panel fails. For ready and retry-ready,
it also verifies the real thumbnail response is HTTP 200 `image/jpeg`, begins
with a JPEG SOI marker, and matches the bundled fixture SHA-256.

## Run from the repository root

Use the same Rust server, WASM bundle, pinned Playwright/PNG decoder install, and browser
installation already prepared by the `Chromium and Firefox against Rust server
and application WASM` workflow job:

```sh
PHOTO_CULLER_SERVER=rust/target/release/photo-culler-server \
PHOTO_CULLER_WEB_ROOT=artifacts/wasm32/bundle \
PHOTO_CULLER_UI_STATES_CAPTURE_DIR=artifacts/ui-states/chromium \
node migration/demo/smoke/ui-states/smoke.mjs chromium
```

Run the same command with `firefox` and a different capture directory. The CI
Firefox runner needs its existing Xvfb wrapper. The script itself creates and
stops an ephemeral Rust server; it does not require `PHOTO_CULLER_API_ORIGIN`.

Artifacts contain ten PNGs per browser: five states at each viewport. Files and
manifests are written only beneath `PHOTO_CULLER_UI_STATES_CAPTURE_DIR`.

## Limitations

The Slint web renderer draws its controls into a canvas, so the retry control has
no dependable DOM button locator in this app. The smoke locates its click by
scanning the gallery canvas column, where `StatusPanel` owns pointer input, and
stops on the observable `error` → `loading` transition. This verifies the real
pointer path but does not claim keyboard/accessibility activation. The hidden
`aria-live` status and visible canvas are both checked. Fixture responses are
intercepted in Playwright; this tests presentation and recovery against the real
WASM app, while the retry's successful response is the real Axum API response.
