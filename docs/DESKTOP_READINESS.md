# Desktop and platform readiness

## Evidence snapshot

This readiness record inspects tracked source at commit
[`ad1bb99f50b0e46dd60dc2bb2b048291f064335a`](https://github.com/scherenhaenden/photo-culler/commit/ad1bb99f50b0e46dd60dc2bb2b048291f064335a)
on 2026-11-18. Inspection ran on a macOS Darwin arm64 host. No build, startup,
or end-to-end command was run for this record. “Tracked” means code or a test
exists in the commit; it does not mean the test passed on the named platform.
See [`BUILDS.md`](BUILDS.md) for recorded executions.

## Evidence by platform

| Platform / surface | Tracked implementation evidence | Build evidence in this record | Startup / complete flow evidence in this record | Current statement |
|---|---|---|---|---|
| Linux x86-64, Python + isolated Chromium | `scripts/build_linux.sh`; `photo_culler/desktop/`; `tests/test_linux_launcher.py`; `tests/e2e/test_linux_desktop_launcher.py`; CI `linux-build-smoke` job | Workflow and build script are tracked; this record contains no artifact or build log | Tests are tracked; this record contains no test run | Source and automated checks are present. Build and startup are unverified in this snapshot record. |
| Linux x86-64, Python + pywebview | Optional `desktop`/`linux` extras in `pyproject.toml`; `photo_culler/desktop/`; `tests/test_desktop.py` | No platform-specific build record in this snapshot | No platform-specific startup or full-flow record in this snapshot | Source exists; packaged desktop readiness is not established here. |
| Linux x86-64, Rust egui | `rust/apps/photo-culler-egui/`; `tests/e2e/test_egui_desktop_launcher.py` | Package is in `rust/Cargo.toml`; this record contains no binary or build log | Test source exists; this record contains no run | Experimental source exists; release readiness and parity are unverified here. |
| Linux x86-64, Rust Tauri | `rust/apps/photo-culler-tauri/`; `tests/e2e/test_tauri_desktop_package.py`; `.github/workflows/ci.yml` package job | Packaging workflow is tracked; this record contains no artifact or build log | Test source exists; this record contains no run | Experimental shell source exists; runtime reliability is unverified here. |
| macOS arm64 | Python desktop source and Rust egui source are tracked | No macOS artifact or build run is recorded in this snapshot | No macOS startup or end-to-end run is recorded | Platform delivery is unverified. |
| WebAssembly / Slint | Roadmap only: `docs/RUST_SLINT_ROADMAP.md` | No Slint crate or WASM build is present in the workspace manifest | No browser startup or flow is recorded | Planned target; not implemented in this snapshot. |
| NAS headless | Python NAS code under `photo_culler/nas/`; Rust NAS modules under `rust/apps/photo-culler-server/src/nas/` | No NAS binary or build record is present in this snapshot | No NAS host acceptance is recorded | Source exists; supported hardware, architecture, package, and deployment acceptance remain open. |

## Promotion checks

For each platform, record these stages separately and cite the exact commit and
artifact:

1. **Compile/package:** command, runner or host OS/architecture, compiler and
   dependency versions, exit code, artifact filename, and SHA-256.
2. **Startup:** clean-install instructions, health/readiness result, log path,
   and clean process shutdown.
3. **Product flow:** fixture/corpus identifier, server/client versions, steps,
   persisted result, and original-file hash comparison.
4. **Support decision:** known limitations, signing/notarization, update path,
   architecture coverage, and owner for the next acceptance run.

Do not derive a single readiness score from source files or test declarations.
An empty execution record is “not run,” not a pass or an estimate.
