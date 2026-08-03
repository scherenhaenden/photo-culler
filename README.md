# photo-culler

> **High-Performance Automated Photo Culling & Modular Analysis Framework**

`photo-culler` is an open-source, decoupled, high-performance photo culling and technical assessment framework designed for high-volume photographers (concerts, portraits, sports, and events). 

It combines compiler-style analysis pipelines, RAW+JPEG logical file pairing, sparse and perceptual hashing, shoot timeline grouping, and multi-tier quality evaluation to streamline photo selection without altering original files.

---

## Architecture Direction: Exchangeable Engines and Frontends

The project deliberately supports parallel technology tracks while product behavior and performance requirements are still being validated. Domain models, catalog persistence, analysis pipelines, scoring, and selection rules remain independent of the presentation layer. Frontends must consume those boundaries instead of embedding culling logic, which lets an implementation be replaced without rewriting the product.

### Engine tracks

| Track | Current maturity | Quality / evidence | Direction |
|---|:---:|---|---|
| **Python analysis engines** | **85% — active** | Modular analyzer registry, cache, normalized image processing, scoring, benchmarks, and broad unit coverage | Reference implementation and current production path |
| **Rust analysis engines** | **3% — contracts only** | Shared request/result/engine contracts compile; no analyzer has been ported or benchmarked | Port individual hot paths only after representative Python benchmarks prove that Rust provides a material end-to-end gain |

A Rust port must preserve the analyzer input/output contract and pass the same validation corpus before replacing a Python implementation. Python remains the orchestration and correctness reference until a Rust engine is measurably faster, equally accurate, and operationally simpler.

### Frontend tracks

| Frontend | Current maturity | What works now | Intended role |
|---|:---:|---|---|
| **FastAPI + HTMX web UI** | **88% — usable** | Dark responsive UI, dashboard, paginated/filterable library, inspector, decisions, keyboard navigation, background analysis progress | Fastest product/design iteration and browser-accessible reference UI |
| **Python + pywebview desktop shell** | **82% — Linux build available** | Native window, random localhost port, session token, host validation, security headers, native bridge, clean shutdown | Current clickable desktop delivery and integration reference |
| **Rust + Tauri + WebGL** | **7% — experimental shell** | Packaged Linux Tauri shell launches an authenticated FastAPI sidecar and has DEB startup coverage; Wayland navigation remains unreliable | Reuse the mature web interaction model with a Rust desktop shell and GPU-accelerated canvas |
| **Rust + egui + wgpu** | **100% — Linux experimental delivery** | Native wgpu window, catalog/gallery browsing, import, thumbnail preview, analysis, decisions, sessions/groups, non-destructive editing and packaged launcher | Complete for the defined experimental Linux scope |

The three frontend directions are intentionally retained. Shared behavior belongs in engines/services; frontend-specific rendering, windowing, and interaction stay in adapters. A future decision should be based on measured catalog size, thumbnail/render latency, installer size, accessibility, platform support, development velocity, and maintenance cost—not language preference alone.

Las releases usan el formato de calendario `yyyy.MM.dd-HH.mm.sss`; véase
[`docs/VERSIONING.md`](docs/VERSIONING.md).

See [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) for replacement boundaries and promotion gates.
See [`docs/TECHNOLOGY_MAP.md`](docs/TECHNOLOGY_MAP.md) for the Python/Rust module map and desktop resource comparison.
See [`docs/FUTURE_INTELLIGENT_WORKFLOW.md`](docs/FUTURE_INTELLIGENT_WORKFLOW.md) for the planned human-approved selection, auto-editing and restoration workflow.
See [`docs/DESKTOP_READINESS.md`](docs/DESKTOP_READINESS.md) for the evidence-based comparison of the two Python desktop paths and the completed easy Linux scope.

### Catalog storage choices

SQLite remains the default because Photo Culler is local-first, needs a zero-administration catalog, and benefits from WAL mode for concurrent UI/background reads. The persistence boundary now accepts either a path or `PHOTO_CULLER_DATABASE_URL`.

| Backend | Readiness | Best fit |
|---|:---:|---|
| **SQLite + WAL** | **90% — default** | Single-user desktop catalogs, removable/local storage, simple backups |
| **PostgreSQL** | **10% — boundary only** | Future multi-user/shared-server catalog; SQLAlchemy URL is recognized, but driver packaging, migrations, CI, and operational testing remain |
| **DuckDB** | **Research** | Analytics/benchmark snapshots, not transactional decision editing |
| **Embedded key/value stores** | **Research** | Metric/thumbnail caches, not the relational source of truth |

Example URL configuration:

```bash
PHOTO_CULLER_DATABASE_URL=sqlite:////absolute/path/catalog.db photo-culler web
```

---

## 📊 Estado de Desarrollo y Cobertura de Código (Coverage Table)

### 🎯 Cobertura Global de Código: **84.0%** (72 Pruebas Superadas)

Measured on Linux/Python 3.14 with statement and branch coverage enabled:

| Módulo / Sub-paquete | Sentencias | Líneas Omitidas | Cobertura % |
|---|:---:|:---:|:---:|
| **`photo_culler/analysis/`** (Analizadores, pipeline, registry y cache) | 477 | 45 | **87.8%** |
| **`photo_culler/catalog/`** (Persistencia SQLite & ORM) | 358 | 13 | **93.5%** |
| **`photo_culler/importing/`** (Galerías, preflight y jobs persistentes) | 427 | 38 | **87.0%** |
| **`photo_culler/editing/`** (Recetas y preview no destructivo) | 162 | 16 | **84.5%** |
| **`photo_culler/cli/`** (Comandos Typer & Formateadores Rich) | 549 | 81 | **80.8%** |
| **`photo_culler/web/`** (FastAPI, servicios, paginación y jobs SSE) | 613 | 98 | **81.3%** |
| **`photo_culler/desktop/`** (Launcher seguro y bridge pywebview) | 97 | 75 | **18.8%** |
| **`photo_culler/core/`** (Modelos de Dominio y Enums) | 96 | 2 | **96.2%** |
| **`photo_culler/grouping/`** (Agrupación timeline) | 34 | 1 | **93.2%** |
| **`photo_culler/bursts/`** (Detección de ráfagas) | 32 | 2 | **88.6%** |
| **`photo_culler/identity/`** (Hashes SHA-256 & dHash) | 58 | 9 | **83.3%** |
| **`photo_culler/pairing/`** (Emparejador RAW/JPEG) | 45 | 0 | **100.0%** |
| **`photo_culler/previews/`** (Generador de thumbnails) | 28 | 5 | **79.4%** |
| **`photo_culler/reports/`** (Generador de reportes) | 19 | 0 | **100.0%** |
| **`photo_culler/scanner/`** (Crawler & filtros de extensión) | 62 | 3 | **94.0%** |
| **`photo_culler/scoring/`** (Scorers técnicos & RAW recovery) | 76 | 2 | **90.8%** |
| **`photo_culler/selection/`** (Reglas de decisión) | 29 | 13 | **51.1%** |
| **`photo_culler/validation/`** (Corpus & benchmark runner) | 39 | 4 | **81.6%** |
| **`photo_culler/volumes/`** (Detector de volúmenes) | 38 | 13 | **66.7%** |
| **TOTAL PROYECTO** | **3,345** | **423** | **84.0%** |

---

## 📈 Tabla de Madurez y Readiness (Updated System Readiness Index)

| Área / Dimensión | Madurez Pre-Web | Madurez Actual | Estado & Avance |
|---|:---:|:---:|---|
| **Arquitectura y Modularidad** | 87% | **95%** | Tubería tipo compilador desacoplada, caché SQLite y arquitectura Web/Desktop unificada |
| **Motor de Análisis & Rendimiento** | 80% | **90%** | Normalización espacial (`max_dim=1920`) y procesamiento en milisegundos |
| **Analizadores Técnicos & ROI** | 74% | **85%** | Evaluación global + ROI central (Subject Zone) para nitidez y clipping |
| **Scoring & Confianza** | 64% | **80%** | Scoring contextual (`concert`, `portrait`) con métrica explícita de confianza |
| **Catálogo & Hashes** | 78% | **88%** | Hashes rápidos/completos, pairing de RAW/JPEG/Sidecars y guardado no destructivo |
| **CLI & Experiencia** | 79% | **88%** | Comandos Typer/Rich completos con `PhotoSelector` y subcomandos `web` y `desktop` |
| **Interfaz Web (FastAPI + HTMX)** | 0% | **88%** | Dark UI, dashboard, biblioteca paginada/filtrable, inspector, navegación y progreso SSE |
| **Aplicación Desktop (pywebview)** | 0% | **82%** | Puerto aleatorio protegido, bridge nativo, cierre limpio y build Linux reproducible |
| **Desktop Rust (Tauri + WebGL)** | 0% | **7%** | Shell experimental con sidecar FastAPI, empaquetado DEB y prueba de arranque; navegación Wayland aún no fiable |
| **Desktop Rust nativo (egui + wgpu)** | 0% | **100%** | Entrega Linux experimental: cliente wgpu, API local, catálogo, importación, análisis, decisiones, sesiones/grupos, edición, paquete y E2E del launcher |
| **Validación Fotográfica Real** | 38% | **70%** | Infraestructura de corpus (`BenchmarkEvaluator`) con F1-score, FRR y FAR |
| **Integración Continua (CI & Testing)** | 70% | **94%** | 72 pruebas Python (incluye Importar → miniatura → Analizar en Chrome real), 2 pruebas Rust y CI Python 3.14 |
| **Readiness para Uso Experimental Real** | 62% | **85%** | Listo para escanear, analizar y clasificar visualmente mediante CLI, Web o Desktop |
| **Readiness Producción con Miles de Fotos** | 35% | **72%** | Modo asistido por UI con atajos de teclado y salvaguardas no destructivas |

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
The percentages above must only be updated from measured test and coverage
output.

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

### 3. Crear y abrir el build clicable de Linux

```bash
uv venv --python 3.14
uv pip install -e '.[linux,build]'
./scripts/build.sh --linux
./builds/linux/photo-culler
```

The generated `builds/linux/photo-culler` is a single executable containing the Python application, templates, and static assets. All PyInstaller output, including temporary work files, stays under `builds/`. Double-clicking it opens an isolated Google Chrome/Chromium app window; closing that window also stops the protected local server. The replaceable build directory never stores the user's catalog: on Linux the desktop catalog lives at `${XDG_DATA_HOME:-~/.local/share}/photo-culler/catalog.db`.

`scripts/build.sh` is the unified local build command. It builds the Linux desktop application by default; choose targets explicitly with `--linux`, `--rust-cli`, `--rust-egui`, or `--rust-tauri`. `--all` builds them all, and exclusions such as `--no-rust-tauri` let you omit one: `./scripts/build.sh --all --no-rust-tauri`. Use `--output /ruta/a/builds` to place final artifacts elsewhere.

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
