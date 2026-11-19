# Build and validation record

This file records executed builds and validation, not build intentions. Every
result must identify the exact commit, date, host platform, command, exit code,
artifact hash, and separate startup/product-flow outcome. CI configuration and
test files are evidence that a check is defined; they are not evidence that it
ran or passed.

## Recorded runs

| Date (UTC) | Commit | Platform / runner | Command | Result | Artifact / hash | Startup / product flow |
|---|---|---|---|---|---|---|
| 2026-11-19 | [`14a4a6fc3371ee5d7ebc8fc3c303e2d7bfa93ceb`](https://github.com/scherenhaenden/photo-culler/commit/14a4a6fc3371ee5d7ebc8fc3c303e2d7bfa93ceb) | GitHub Actions `ubuntu-24.04`, `macos-15`, `windows-2025` | [Rust Slint demo artifacts #44508069950](https://github.com/scherenhaenden/photo-culler/actions/runs/44508069950), workflow dispatch on `feature/sprint-01-photo-culler-pro-design` | All 6 jobs passed: staging contracts, Linux, macOS arm64, Windows x86-64, WASM, Chromium/Firefox. | Five artifacts; GitHub SHA-256 digests and binary hashes are listed below. | Native packages build/stage and the owned server child starts, serves gallery/JPEG and exits in lifecycle tests. Native `runtime_smoke` remains `not_run`; no packaged Slint window was opened. Chromium and Firefox ran the actual Axum+WASM Library flow and produced reviewed 1440×900 screenshots. |
| 2026-11-19 | [`202baa2c3efb8a4fa81a20d02c06af6322da1bc4`](https://github.com/scherenhaenden/photo-culler/commit/202baa2c3efb8a4fa81a20d02c06af6322da1bc4) | Darwin 26.6.2 arm64, local host | `cargo test --offline --locked --manifest-path rust/Cargo.toml -p photo-culler-server --test s01_runtime` | 4 passed, 0 failed. | Local test binary only; no package artifact created. | Health, gallery, JPEG, allowlisted bundle/MIME and standalone/owned shutdown tests ran. This is API/process evidence, not native GUI evidence. |

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
