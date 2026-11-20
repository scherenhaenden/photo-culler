# Build and validation record

This file records executed builds and validation, not build intentions. Every
result must identify the exact commit, date, host platform, command, exit code,
artifact hash, and separate startup/product-flow outcome. CI configuration and
test files are evidence that a check is defined; they are not evidence that it
ran or passed.

## Recorded runs

| Date (UTC) | Commit | Platform / runner | Command | Result | Artifact / hash | Startup / product flow |
|---|---|---|---|---|---|---|
| 2026-11-19 | [`3884dc47feea0d29d691c9f5431343b624537538`](https://github.com/scherenhaenden/photo-culler/commit/3884dc47feea0d29d691c9f5431343b624537538) | GitHub Actions `ubuntu-24.04`, `macos-15`, `windows-2025` | [Candidate documentation validation #44566993573](https://github.com/scherenhaenden/photo-culler/actions/runs/44566993573), PR #157 | **6/6 jobs passed.** | This documentation-only commit did not produce new application artifacts. See the exact code/artifact provenance from #44563618333 below. | Repeated all six build, WASM, native and browser jobs successfully on the same application tree. The sprint integration branch still requires the designated merge decision and a fresh run on its resulting SHA. |
| 2026-11-19 | [`59cb9f01746b4f66f180f8c9c2927489a8f7e5e7`](https://github.com/scherenhaenden/photo-culler/commit/59cb9f01746b4f66f180f8c9c2927489a8f7e5e7) | GitHub Actions `ubuntu-24.04`, `macos-15`, `windows-2025` | [Integrated Rust/Slint acceptance #44563618333](https://github.com/scherenhaenden/photo-culler/actions/runs/44563618333), stacked PR #157 | **6/6 jobs passed:** build/staging contracts, Linux x86-64, macOS arm64, Windows x86-64, WASM, Chromium/Firefox. | Eight artifacts; GitHub archive SHA-256 values and screenshot hashes are listed below. S05 browser captures: 1440×900 `20483b0f82004ffcebdcf332323264bbd591324c25a0bfc4bd896c3a0db7065d`; 1100×700 `240cc1bc576a71f4a737b8e2e1ce51873ccf3af546d74aaa505003b97e827d01`. | All five targets passed. The browsers captured loading, empty, typed 503, retry-loading and retry-ready at both viewports; native runners displayed live one-photo and ten-card synthetic Library screens, then verified owned server close/reopen and external-server survival. |
| 2026-11-19 | [`68959bf702c554839b720d80fd51bfbad6824317`](https://github.com/scherenhaenden/photo-culler/commit/68959bf702c554839b720d80fd51bfbad6824317) | GitHub Actions `ubuntu-24.04`, `macos-15`, `windows-2025` | [Native S01 acceptance #44546749693](https://github.com/scherenhaenden/photo-culler/actions/runs/44546749693), PR #156 based on design/docs `51deca2` | 6/6 jobs passed: contracts, Linux, macOS arm64, Windows x86-64, WASM, Chromium/Firefox. | Three platform package artifacts, three native GUI evidence artifacts, WASM and browser artifacts; GitHub SHA-256 digests are recorded below. Native PNG SHA-256: Linux `8ddf029db66049b8fa3d338c317869c833c38abd9612e6a87c302695a9aa9996`; macOS `dfb10ff595c3b75c5fbd3f93cce8e5b398b57f4310ccd5e2000e2d322fe1832f`; Windows `1b563e43abc74cbc52c709f89072131e2e73f6cc2383324de7f2aa27a489173e`. | Each desktop runner displayed the live one-photo JPEG and separately the ten-card synthetic Pro fixture; first launch/reopen closed cleanly and released the owned port, while an explicit external server remained healthy. Windows DWM crop excludes taskbar and desktop margins. |
| 2026-11-19 | [`afe088242bdcf173eb8e83c9571bf598f52c4e86`](https://github.com/scherenhaenden/photo-culler/commit/afe088242bdcf173eb8e83c9571bf598f52c4e86) | Darwin 26.6.2 arm64, local host; Chromium 134.0.6998.35 | `cargo test --offline --locked -p photo-culler-slint --lib`; native and WASM `cargo check`; Python parity contract; Node capture smoke | 33 passed, 0 failed, 1 ignored; 8/8 parity assertions; native/WASM checks, formatting, syntax and diff checks passed. No Actions run was visible yet for this SHA. | Paired deterministic reference/candidate captures and SHA-256 manifest are committed in [`evidence/design`](rust-slint-demo/evidence/design/README.md). Candidate hashes: `a7ae64afa5b73abda9e0e4b4e96e2cfd2f23d062f664ad66bf11ff1ea6847714` (1440×900), `d99f55a69872fc0b8ec96046313dd70f34b66c0374467a8c1461c9759dab8b2f` (1100×700). | The Slint WASM screen rendered ten smoke records at 5×2 and 4×3 layouts. All records reuse one bundled JPEG and explicit synthetic metadata. This is not native-window evidence. |
| 2026-11-19 | [`14a4a6fc3371ee5d7ebc8fc3c303e2d7bfa93ceb`](https://github.com/scherenhaenden/photo-culler/commit/14a4a6fc3371ee5d7ebc8fc3c303e2d7bfa93ceb) | GitHub Actions `ubuntu-24.04`, `macos-15`, `windows-2025` | [Rust Slint demo artifacts #44508069950](https://github.com/scherenhaenden/photo-culler/actions/runs/44508069950), workflow dispatch on `feature/sprint-01-photo-culler-pro-design` | All 6 jobs passed: staging contracts, Linux, macOS arm64, Windows x86-64, WASM, Chromium/Firefox. | Five artifacts; GitHub SHA-256 digests and binary hashes are listed below. | Native packages build/stage and the owned server child starts, serves gallery/JPEG and exits in lifecycle tests. Native `runtime_smoke` remains `not_run`; no packaged Slint window was opened. Chromium and Firefox ran the actual Axum+WASM Library flow and produced reviewed 1440×900 screenshots. |
| 2026-11-19 | [`202baa2c3efb8a4fa81a20d02c06af6322da1bc4`](https://github.com/scherenhaenden/photo-culler/commit/202baa2c3efb8a4fa81a20d02c06af6322da1bc4) | Darwin 26.6.2 arm64, local host | `cargo test --offline --locked --manifest-path rust/Cargo.toml -p photo-culler-server --test s01_runtime` | 4 passed, 0 failed. | Local test binary only; no package artifact created. | Health, gallery, JPEG, allowlisted bundle/MIME and standalone/owned shutdown tests ran. This is API/process evidence, not native GUI evidence. |
| 2026-11-19 | [`98d9b6af7ac31dd807ddf1a4d204dae0df07a212`](https://github.com/scherenhaenden/photo-culler/commit/98d9b6af7ac31dd807ddf1a4d204dae0df07a212) | Darwin 26.6.2 arm64, local host | `cargo test --offline --locked --manifest-path rust/Cargo.toml -p photo-culler-slint --lib` | 26 passed, 0 failed, 1 ignored (the ignored test requires a staged server executable). | Local test binary only; no package artifact created. | Controller loading/ready/empty/error/retry and native transport/owned-server behaviors passed; this is not a GUI capture. |

### Workflow artifacts for integrated run #44563618333

All eight artifacts were produced from source SHA
`59cb9f01746b4f66f180f8c9c2927489a8f7e5e7`. Artifact hashes below are the
GitHub SHA-256 digest of each uploaded archive. The current native GUI
`gallery-smoke.png` images use the ten-card synthetic fixture; dimensions and
PNG hashes are recorded in the adjacent design evidence README.

| Artifact | GitHub artifact SHA-256 | Contents / SHA-256 |
|---|---|---|
| `rust-slint-demo-linux-x64-44563618333-1` | `3ff8c0416bf2ba61cbad7cd21feef070099bc801347464e7b29e2ef952876759` | Linux desktop/server package and checksums. |
| `rust-slint-demo-macos-arm64-44563618333-1` | `b2d5bf59276917675cd279ce5d094261fac0f07fb10697a4ba279e059e302741` | macOS arm64 desktop/server package and checksums. |
| `rust-slint-demo-windows-x64-44563618333-1` | `5c96f818731da5f7bcd01a7a462125bbd026bc3d1a5ee1b863bbaf87626bb698` | Windows x86-64 desktop/server package and checksums. |
| `rust-slint-demo-wasm32-44563618333-1` | `2471b22d37f72a491614678372a1e28fe64a3d67af8783b8d5bf6c794bd8829d` | WASM bundle, manifest and checksums. |
| `rust-slint-demo-browser-acceptance-44563618333-1` | `4533c63f6935aecdcf6acb7a74b536c1d4edfd85b8f60ee23083c32a6c3ca3cb` | 20 Chromium/Firefox UI-state screenshots, two S05 design screenshots, manifests and hashes. |
| `rust-slint-demo-native-gui-linux-x64-44563618333-1` | `90ff19b24b6379950749a0ac7779144dc4cade75735bc0b89219f30d31492693` | Linux live and synthetic gallery screenshots, readiness/lifecycle reports and logs. |
| `rust-slint-demo-native-gui-macos-arm64-44563618333-1` | `0cd28a1c7dc31aaace2d3ebc7453163a113c2463851ab154b4c8a0c1a0ff8d51` | macOS live and synthetic gallery screenshots, readiness/lifecycle reports and logs. |
| `rust-slint-demo-native-gui-windows-x64-44563618333-1` | `cbe97bd4a13b2e31bb9ea3f8457e6551120d9cf642395a7b7b70d5f8cf07d4ff` | Windows live and synthetic gallery screenshots, readiness/lifecycle reports and logs. |

Native `gallery-smoke.png` SHA-256 and dimensions: Linux x86-64 1439×880
`d193b2a17eb9d0667fe121928eaaacdc2b255c68f63d06f77cf31fd57d93224d`;
macOS arm64 1212×840
`3aff5ce1cc4690a5febf2dfcd53c54b82ef76928fe75e4356975a8c4101225b0`;
Windows x86-64 1442×932
`22ce0f81c5bbc9e36db2d6e876213966521b706cd6fbf0e4c588f97fbae8779f`.
The workflow page links to all artifacts and their expiration dates.

### Workflow artifacts for run #44508069950

GitHub exposes these artifacts on the linked run. The artifact digest is GitHub's
SHA-256 for the uploaded archive. Native and WASM contents also carry
`checksums.sha256` and `artifact-manifest.json` with the tested source SHA and
`runtime_smoke` status.

| Artifact | GitHub artifact SHA-256 | Contents / SHA-256 |
|---|---|---|
| `rust-slint-demo-linux-x64-44508069950-1` | `313bee17e18a7fa0b3ebf216e080b796995c5457af7f1a4408869c8c4cddeebb` | `photo-culler-desktop` `6b2a48d436ade89477862fb59ff07fe9656e3b7cf058ceae81c390b028769564`; `photo-culler-server` `849218a5c1773986402536d60a9682a1e703094edc6b89a182015ff30d6bec29` |
| `rust-slint-demo-macos-arm64-44508069950-1` | `e9be8357d5e0470e26a4a39e9a8dfc28ddca58c8edfe0a7f7a88befa57c6652d` | `photo-culler-desktop` `e852470dfe4741d5ecd46aee1a86b450b7817691e191f73c698e329f8d5469c9`; `photo-culler-server` `ab86361b00748bfbdd6259afac143884464bbb9bb5387bb47e13873626f7176b` |
| `rust-slint-demo-windows-x64-44508069950-1` | `81e4d124cd389295e3f5d0672e57ee6f56afc81fbea6261e4bbe58d35d264993` | `photo-culler-desktop.exe` `f106c9c5e15adbf0b82ab5095a50ac074ca4e7f71b09d25def1aba4974866f62`; `photo-culler-server.exe` `42d71348fa2b8ded3c7ad2c806272a4ff2e0bdd0eba3b257ef577b3ce57e0982` |
| `rust-slint-demo-wasm32-44508069950-1` | `b2c956af53e0f0f2f3af917f9733f4e747b4afac14a18974c230748234648e93` | `photo_culler_slint_bg.wasm` `37259de6297d712421319804de3f15a191175422478a46d935e89ca84117009d`; `bundle.json` `576d681b4715faaf75a44e50a5c441c577a8805f6210f387fd027c125d33b703` |
| `rust-slint-demo-browser-acceptance-44508069950-1` | `4cee465119dd195e63b65f5592d8989c3a7cbd7fb50b82f5bc654c5a4d82a03d` | `chromium.png` `21a701439a5c95b759d7a087b71a8850bbd71e34797b6f820acc109390b79e13`; `firefox.png` `2ce1d74e5bd68d0ef0305e7d5ec4e4aa1c10624f0dd07bc7d4bdaeaa26f8a393` |

### Workflow artifacts for run #44546749693

The GitHub archive digest identifies each uploaded ZIP. Native GUI artifacts
contain `evidence.json`, separate live one-photo and ten-card screenshots, and
process/server lifecycle logs. The 10-card PNG hashes also appear in the native
manifest in [`evidence/design`](rust-slint-demo/evidence/design/README.md).

| Artifact | GitHub artifact SHA-256 | Contents |
|---|---|---|
| `rust-slint-demo-linux-x64-44546749693-1` | `e179f3358c24b5c399f9a0196f4ebca6e030bd90c4dbb3ca223e9a8c8b4fdf37` | Linux x86-64 desktop/server package and checksums. |
| `rust-slint-demo-macos-arm64-44546749693-1` | `f68a2bfd967ee60fe378f09aabe56b08dae3d8ad45c40767e1a3245f37627736` | macOS arm64 desktop/server package and checksums. |
| `rust-slint-demo-windows-x64-44546749693-1` | `c5b379a97c208570c16ad02dd9a782f94d97b32482997f766fdc7b8716c01251` | Windows x86-64 desktop/server package and checksums. |
| `rust-slint-demo-wasm32-44546749693-1` | `adf1838179491690a7fb487012091e6a1fa83343223b968e0da857c13d1fc326` | WASM bundle, manifest and checksums. |
| `rust-slint-demo-browser-acceptance-44546749693-1` | `7ed5dffb6f8b224328c4f184b305b8f7aadbe34a31c193a27fbf1bee941acbb2` | Chromium and Firefox screenshots, result metadata and hashes. |
| `rust-slint-demo-native-gui-linux-x64-44546749693-1` | `75fa527f3eae5bb9e76840103b24b4aba36116550b7deb92896dd1319fb1ee73` | Linux native first/reopen/external-server evidence; smoke capture 1439×880. |
| `rust-slint-demo-native-gui-macos-arm64-44546749693-1` | `f5f2c1fa203bb9309b8aab26bbbd912cd8e38f57f4737d597cd8d0eaa22c0d5a` | macOS native first/reopen/external-server evidence; smoke capture 1212×840. |
| `rust-slint-demo-native-gui-windows-x64-44546749693-1` | `1f81f8292e4ce554cec66f149ced909d751e1aa3488d3cddebc9866fff53abea` | Windows native first/reopen/external-server evidence; DWM smoke capture 1442×932. |

The artifact manifests report visible Cargo version `2026.11.18+18.43.40`,
Rust/Cargo `1.100.0`, targets `x86_64-unknown-linux-gnu`,
`aarch64-apple-darwin`, `x86_64-pc-windows-msvc` and
`wasm32-unknown-unknown`. The demo build ID and eventual release version use
their documented ISO date formats; this feature sprint did not change Cargo's
SemVer-compatible workspace version.

## Repository snapshot reviewed

| Date | Commit | Platform inspected | Evidence | Result |
|---|---|---|---|---|
| 2026-11-18 | [`ad1bb99f50b0e46dd60dc2bb2b048291f064335a`](https://github.com/scherenhaenden/photo-culler/commit/ad1bb99f50b0e46dd60dc2bb2b048291f064335a) | macOS Darwin arm64 | `rust/Cargo.toml`, `.github/workflows/ci.yml`, `.github/workflows/release.yml`, `scripts/`, `tests/`, and platform source paths | Read-only inventory; not a build or test result. |

## Defined commands (not run in this record)

| Platform | Build/check definition | Tracked startup/flow check | Execution status |
|---|---|---|---|
| Python/Linux x86-64 | `./scripts/build_linux.sh`; CI `linux-build-smoke` | `pytest -m e2e tests/e2e/test_linux_desktop_launcher.py` | Defined in source/CI; not run for this record. |
| Rust/Linux x86-64 | `cargo build --locked --release --manifest-path rust/Cargo.toml --package photo-culler-egui --bin photo-culler-egui --target x86_64-unknown-linux-gnu` | `tests/e2e/test_egui_desktop_launcher.py` exists | Build and test not run for this record. |
| Rust/macOS arm64 | `cargo build --locked --release --manifest-path rust/Cargo.toml --package photo-culler-egui --bin photo-culler-egui --target aarch64-apple-darwin` | No macOS startup test identified in this snapshot | Build and test not run for this record. |
| Rust/WASM | No Slint package in `rust/Cargo.toml` | No browser startup test for the target | Not buildable from this snapshot. |
| Rust/NAS | No standalone server package in `rust/Cargo.toml` | NAS acceptance remains a deployment check | Not buildable from this snapshot. |

## Adding a run

Add a row under **Recorded runs** only after executing the command. Use an
immutable commit hash; include runner image/host and installed native packages;
record stdout/stderr or a durable CI URL; and compute SHA-256 from the output
artifact. Mark smoke test and complete product flow separately, with the
fixture/catalog identity and original-file integrity result. A failed or
skipped run should keep its actual exit status and reason.

## Reproduce the paired S05 browser captures

The capture script requires a built `photo-culler-server`, staged WASM bundle,
Node.js, and Playwright 1.51.1. From the repository root:

```sh
npm install --no-save --package-lock=false playwright@1.51.1
PHOTO_CULLER_SERVER=rust/target/release/photo-culler-server \
PHOTO_CULLER_WEB_ROOT=artifacts/wasm32/bundle \
PHOTO_CULLER_GALLERY_CAPTURE_DIR=artifacts/s05-gallery \
node rust/apps/photo-culler-slint/ui/library/capture_smoke.mjs
```

The script opens the local Rust server, loads `?photo-culler-smoke=1`, verifies
that the Slint canvas follows 1440×900 and 1100×700 CSS and backing-store
dimensions, checks the REST JPEG hash, and writes both PNGs plus a JSON
manifest. The fixture's ten records have synthetic metadata and repeat one
image; this is not real-catalogue, native-window, or full-product acceptance.
