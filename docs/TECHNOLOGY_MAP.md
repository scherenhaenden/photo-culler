# Technology map and evidence snapshot

## Snapshot scope

This map describes tracked source at commit
[`ad1bb99f50b0e46dd60dc2bb2b048291f064335a`](https://github.com/scherenhaenden/photo-culler/commit/ad1bb99f50b0e46dd60dc2bb2b048291f064335a),
dated 2026-11-18. It is an inventory of repository evidence, not a statement
that every path was built or exercised on each platform. See
[`BUILDS.md`](BUILDS.md) for execution records and
[`DESKTOP_READINESS.md`](DESKTOP_READINESS.md) for platform-specific gates.

## Component map

| Area | Python evidence in snapshot | Rust evidence in snapshot | Status supported by source |
|---|---|---|---|
| Domain and catalog | `photo_culler/core/`, `identity/`, `catalog/` | `rust/crates/photo-culler-core/src/lib.rs`; server catalog code under `rust/apps/photo-culler-server/src/catalog/` | Python implementation and Rust domain/catalog modules are tracked. This does not prove schema parity or deployment. |
| Import and organization | `importing/`, `scanner/`, `pairing/`, `grouping/`, `bursts/`, `sessions/`, `volumes/` | Server import modules under `rust/apps/photo-culler-server/src/import/` | Both code paths are present. The Rust server package is not a workspace member on this snapshot. |
| Analysis and selection | `analysis/`, `scoring/`, `selection/`, `learning/`, `validation/` | Core analysis contract in `rust/crates/photo-culler-core/`; image primitives in `rust/crates/photo-culler-image-engine/` | Python path is the implemented reference. Rust analyzer parity and corpus agreement are not evidenced here. |
| Image and editing | `previews/`, `metadata/`, `editing/`, `restoration/`, `generative/` | `photo-culler-image-engine` has metric primitives and `image` JPEG/PNG/TIFF feature declarations | The Rust crate is not evidence of a complete image or RAW pipeline. Its codec feature list does not include a RAW decoder. |
| API and web UI | `photo_culler/web/` routes, services, templates and static files | Rust server HTTP modules are tracked under `rust/apps/photo-culler-server/src/http/` | Python web is the current reference implementation. Rust route modules require integration and platform validation. |
| Desktop | `photo_culler/desktop/`, `scripts/build_linux.sh`, desktop tests under `tests/` | `rust/apps/photo-culler-egui/` and `rust/apps/photo-culler-tauri/` | Python and Rust desktop source exists. Source presence alone does not establish package readiness, parity, or clean shutdown. |
| CLI and NAS | `photo_culler/cli/`, `photo_culler/nas/` | `rust/apps/photo-culler-cli/`; Rust NAS and server modules under `rust/apps/photo-culler-server/src/nas/` | Python commands and NAS path are tracked. Rust CLI/server runtime wiring is not established by the workspace manifest. |
| Slint/WASM | No Slint implementation is tracked | No `photo-culler-slint` crate or generated WASM frontend is present | Target architecture only; no implementation or build evidence in this snapshot. |

## Evidence limits

- The snapshot workspace lists the core, image-engine, CLI, egui, and Tauri
  crates in `rust/Cargo.toml`. It does not list the server crate or a Slint
  crate. Inspect the manifest before treating server source as a runnable app.
- Rust `.rs` modules and Python package directories establish tracked source,
  not successful builds, correctness, startup, or cross-platform behavior.
- The repository contains CI definitions and tests. They are not execution
  records; exact build and test runs belong in [`BUILDS.md`](BUILDS.md).
- No resource or performance benchmark is asserted here. Record the machine,
  OS, architecture, corpus, command, commit, result, and raw output before
  comparing memory, latency, or throughput.

## Evidence record

| Snapshot date | Commit | Platform inspected | Inspection performed | Result |
|---|---|---|---|---|
| 2026-11-18 | `ad1bb99f50b0e46dd60dc2bb2b048291f064335a` | macOS Darwin arm64 | Tracked-path and manifest review | Source inventory only; no application build or runtime test is claimed. |
