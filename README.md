# photo-culler

> **High-Performance Automated Photo Culling & Modular Analysis Framework**

`photo-culler` is an open-source, decoupled, high-performance photo culling and technical assessment framework designed for high-volume photographers (concerts, portraits, sports, and events). 

It combines compiler-style analysis pipelines, RAW+JPEG logical file pairing, sparse and perceptual hashing, shoot timeline grouping, and multi-tier quality evaluation to streamline photo selection without altering original files.

---

## Architecture Direction: Exchangeable Engines and Frontends

The [Rust and Slint migration roadmap](docs/RUST_SLINT_ROADMAP.md) defines the target architecture and the 64 implementation tasks for shared native and WebAssembly interfaces over REST.

The project deliberately supports parallel technology tracks while product behavior and performance requirements are still being validated. Domain models, catalog persistence, analysis pipelines, scoring, and selection rules remain independent of the presentation layer. Frontends must consume those boundaries instead of embedding culling logic, which lets an implementation be replaced without rewriting the product.

### Engine tracks

| Track | Snapshot status | Source evidence | Direction |
|---|:---:|---|---|
| **Python analysis engines** | Implemented reference path | `photo_culler/analysis/`, `scoring/`, `validation/`, and `tests/` | Preserve as the comparison implementation while migration behavior is validated |
| **Rust analysis engines** | Contracts and image primitives tracked | `rust/crates/photo-culler-core/` and `rust/crates/photo-culler-image-engine/`; no analyzer parity result is recorded | Port individual hot paths only after the same corpus demonstrates a measured end-to-end gain |

A Rust port must preserve the analyzer input/output contract and pass the same validation corpus before replacing a Python implementation. Python remains the orchestration and correctness reference until a Rust engine is measurably faster, equally accurate, and operationally simpler.

### Frontend tracks

| Frontend | Snapshot status | Tracked evidence | Intended role |
|---|:---:|---|---|
| **FastAPI + HTMX web UI** | Python reference UI is tracked | `photo_culler/web/` routes, templates, static assets, and web tests | Browser-accessible reference for behavior during migration |
| **Python desktop shells** | Linux launcher and pywebview source are tracked; current run status is not recorded here | `photo_culler/desktop/`, `scripts/`, and desktop tests | Desktop packaging and platform behavior must be reported from dated runs |
| **Rust + Tauri** | Experimental shell source is tracked; runtime status is not recorded here | `rust/apps/photo-culler-tauri/` and package test/CI definitions | Candidate for reuse of the web interaction model after runtime validation |
| **Rust + egui + wgpu** | Experimental shell source is tracked; runtime status is not recorded here | `rust/apps/photo-culler-egui/` and launcher test source | Candidate native UI; parity and package readiness remain to be verified |

The frontend implementations remain separate code paths. Shared behavior belongs in engines/services; frontend-specific rendering, windowing, and interaction stay in adapters. Any promotion decision should cite runs for catalog size, thumbnail/render latency, installer size, accessibility, platform support, development effort, and maintenance cost.

This table describes tracked source at snapshot commit [`ad1bb99f50b0e46dd60dc2bb2b048291f064335a`](https://github.com/scherenhaenden/photo-culler/commit/ad1bb99f50b0e46dd60dc2bb2b048291f064335a), dated 2026-11-18. It is not a build or runtime report. See [`docs/TECHNOLOGY_MAP.md`](docs/TECHNOLOGY_MAP.md), [`docs/DESKTOP_READINESS.md`](docs/DESKTOP_READINESS.md), and [`docs/BUILDS.md`](docs/BUILDS.md) for evidence scope and platform execution records.

Las releases usan el formato de calendario `yyyy.MM.dd-HH.mm.sss`; véase
[`docs/VERSIONING.md`](docs/VERSIONING.md).

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for replacement boundaries and promotion gates.
See [`docs/TECHNOLOGY_MAP.md`](docs/TECHNOLOGY_MAP.md) for the Python/Rust module map and evidence snapshot.
See [`docs/FUTURE_INTELLIGENT_WORKFLOW.md`](docs/FUTURE_INTELLIGENT_WORKFLOW.md) for the planned human-approved selection, auto-editing and restoration workflow.
See [`docs/DESKTOP_READINESS.md`](docs/DESKTOP_READINESS.md) for the platform readiness evidence matrix and [`docs/BUILDS.md`](docs/BUILDS.md) for dated build and validation runs.

### Catalog storage choices

SQLite remains the default because Photo Culler is local-first, needs a zero-administration catalog, and benefits from WAL mode for concurrent UI/background reads. The persistence boundary now accepts either a path or `PHOTO_CULLER_DATABASE_URL`.

| Backend | Tracked implementation status | Best fit |
|---|:---:|---|
| **SQLite + WAL** | Default persistence path in the current Python catalog | Local-first catalogs and removable storage; backup behavior is documented separately |
| **PostgreSQL** | URL boundary only; deployment readiness is not established | Future multi-user/shared-server use after driver packaging, migrations, CI, and operational validation |
| **DuckDB** | **Research** | Analytics/benchmark snapshots, not transactional decision editing |
| **Embedded key/value stores** | **Research** | Metric/thumbnail caches, not the relational source of truth |

Example URL configuration:

```bash
PHOTO_CULLER_DATABASE_URL=sqlite:////absolute/path/catalog.db photo-culler web
```

---

## Development and validation evidence

This repository snapshot contains tests under `tests/`, including unit and
end-to-end test sources. This README does not carry a dated coverage report,
immutable source commit, or raw coverage output, so it does not publish a
coverage total or maturity score. See [`docs/BUILDS.md`](docs/BUILDS.md) for
the evidence currently recorded and the fields required for a future run.

---

## 💻 Interfaz Web & Escritorio (Web UI & Desktop GUI)

### Gallery import milestone

The web library now has an honest empty state and a local-folder import flow.
The versioned `/api/v1/galleries` and `/api/v1/import-jobs` contracts create
logical galleries, index supported files without copying originals, persist
progress/cancellation state, pause and resume cooperatively, recover interrupted
jobs as resumable after restart, and avoid duplicates on a rescan. Recent jobs
remain visible in the library so their controls survive a page or application
reload. Import is a single-action workflow: the first click creates or selects
the gallery and immediately starts indexing, without a misleading confirmation
stage. Analysis started during an active import waits for it and then reads the
updated catalog automatically. A read-only preflight API remains available to
other clients; the library keeps an explicit active gallery and can add multiple
source folders to it. File symlinks inside a source are skipped by default so
discovery cannot escape the selected tree. SQLite schema changes are tracked in
`schema_migrations`.

Each import now owns a persisted scan revision. Rescans report newly discovered,
modified, moved and newly missing files. Quick hashes preserve logical photo
identity for unambiguous moves without collapsing identical files that still
exist at separate paths. Unavailable sources and their files are marked
`offline`, not deleted or incorrectly reported as missing. Source-relative glob
exclusions are persisted and reused by preflight, import, resume and rescan.

Technical analysis coordination is application-scoped rather than process
global. Progress fan-out uses bounded queues, the production worker contains no
artificial sleeps, and pause, resume, cancel and shutdown are cooperative at
photo boundaries. Analysis-job persistence and viewport-priority scheduling
remain separate follow-up milestones.

The first non-destructive editing vertical is functional. Versioned SQLite
recipes persist exposure, white-balance temperature and tint; undo/redo survives
restart; interactive requests render real low/high-resolution JPEG previews
from the original while an LRU stores only disposable preview bytes. The source
file is never written, and tests compare its full hash before and after edits.

This is **partially implemented**, not a complete RAW workflow: resume after a
process restart performs a fresh idempotent scan of the persisted source,
full hashes still run only through explicit identity tools rather than a
background tier. The current white-balance transform is an initial deterministic
preview approximation, not a color-managed RAW pipeline. Export, PostgreSQL
operations, Tauri, advanced color management and local masks remain
planned or prototype-only.
Record future test, coverage, and performance results in [`docs/BUILDS.md`](docs/BUILDS.md) with the exact commit, date, and platform.

### 1. Iniciar Servidor Web Local (`FastAPI` + `HTMX`)

```bash
# Iniciar servidor web y abrir navegador automáticamente en http://127.0.0.1:8765
photo-culler web
```

### 2. Iniciar Aplicación de Escritorio (`pywebview`)

```bash
# Lanzar ventana nativa de escritorio (sin navegador externo)
photo-culler desktop
```

### 3. Crear y abrir builds para macOS y Linux

```bash
# macOS (Genera ejecutables en builds/{tipo}/mac/)
./scripts/build_mac.sh

# Linux (Genera ejecutable en builds/desktop/linux/)
./scripts/build.sh --linux
```

Los ejecutables se organizan en la estructura estandarizada **`builds/{tipo}/{sistema}/`**:
- **macOS Desktop (`pywebview`)**: `builds/desktop/mac/photo-culler`
- **macOS Rust GUI (`egui` + Metal)**: `builds/rust-egui/mac/photo-culler-egui`
- **macOS Rust CLI**: `builds/rust-cli/mac/photo-culler-cli`
- **macOS Web Server**: `builds/web/mac/photo-culler-web`

Véase [`docs/BUILDS.md`](docs/BUILDS.md) para el registro de builds y validación.


### 4. Rust workspace, CLI, and frontend bootstraps

```bash
cd rust
cargo test --workspace
cargo run -p photo-culler-cli -- status
cargo run -p photo-culler-cli -- backends
cargo run -p photo-culler-cli -- frontends
```

The Rust workspace keeps Tauri/WebGL as a bootstrap. The egui/wgpu client is a functional native alpha that connects to the local application API; its README documents the supported workflow and remaining parity gaps.

### ⌨️ Atajos de Teclado en el Visor
- `1`: Marcar como **BEST** (Verde)
- `2`: Marcar como **KEEP** (Azul)
- `3`: Marcar como **ALTERNATE**
- `4`: Marcar como **REVIEW** (Amarillo)
- `X`: Marcar como **REJECT** (Rojo, no destructivo)
- `R`: Marcar como **RECOVER**

---

## ⚡ Quick Start & Development Tools

### 1. Instalar Dependencias de Desarrollo

```bash
# Crear entorno virtual e instalar paquete con soporte web y desktop
python3 -m venv .venv
source .venv/bin/activate
pip install -e .[desktop] ruff mypy pytest-cov coverage httpx
```

### 2. Ejecutar Linters y Formateador (Ruff & Mypy)

```bash
# Comprobar reglas de linter con Ruff
ruff check photo_culler/ --fix

# Formatear código
ruff format photo_culler/ tests/

# Verificación estática de tipos con Mypy
mypy photo_culler/
```

### 3. Ejecutar Pruebas Unitarias y Cobertura (Coverage)

```bash
# Cobertura en terminal
pytest --cov=photo_culler --cov-report=term-missing

# Flujo end-to-end real en Chrome (usa /usr/bin/google-chrome)
pytest -m e2e tests/e2e
```

---

## 📄 License

Distributed under the MIT License. See `LICENSE` for details.
