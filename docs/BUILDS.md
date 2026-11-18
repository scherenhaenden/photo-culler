# Build and validation record

This file records executed builds and validation, not build intentions. Every
result must identify the exact commit, date, host platform, command, exit code,
artifact hash, and separate startup/product-flow outcome. CI configuration and
test files are evidence that a check is defined; they are not evidence that it
ran or passed.

## Recorded runs

| Date (UTC) | Commit | Platform / runner | Command | Result | Artifact / hash | Startup / product flow |
|---|---|---|---|---|---|---|
| — | — | — | — | No build or runtime execution was recorded by task 63. | — | — |

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
