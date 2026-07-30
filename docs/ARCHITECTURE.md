# Architecture and Replacement Boundaries

## Principle

Photo Culler is a set of domain and analysis capabilities with replaceable delivery adapters. A frontend may request work and render results, but it must not own photo identity, catalog persistence, scoring, or selection policy. An engine may calculate metrics, but it must not depend on a specific UI.

## Current boundaries

```text
Web / pywebview / future Tauri / future egui
                      |
              application services
                      |
       catalog | jobs | analysis contracts
                      |
       Python engine | future Rust engines
```

- `photo_culler/core` owns stable domain types and enums.
- `photo_culler/catalog` owns persistence and repository boundaries.
- `photo_culler/analysis` owns analyzer contracts, the registry, pipeline, results, and cache.
- `photo_culler/scoring` and `photo_culler/selection` own product decisions.
- `photo_culler/web/services` adapts domain capabilities for presentation.
- `photo_culler/web` and `photo_culler/desktop` are delivery adapters.
- `photo_culler/importing` owns gallery/import orchestration. Its coordinator is
  constructed per FastAPI application and persists every job transition; no
  frontend or process-global singleton owns import state.

## Gallery import milestone

The first gallery contract is deliberately narrow and operational:

- `Gallery` is a logical collection and `ImportSource` is a physical folder;
- imports index in place and never copy, rename, or modify originals;
- a read-only preflight scans supported extensions, bytes, and logical pairing
  groups before an import job is created;
- gallery/source uniqueness and stable source-relative photo identity make a
  rescan idempotent;
- import jobs and counters survive process restarts, while pause and
  cancellation are cooperative at file/photo boundaries;
- interrupted jobs reopen as paused and explicitly resumable; a recovered
  worker performs a fresh idempotent scan rather than pretending to retain an
  in-memory directory iterator;
- `/api/v1` DTOs carry an explicit contract version and do not expose ORM rows;
- the library selects an active logical gallery and scopes catalog queries to
  it instead of mixing photos from unrelated collections;
- file symlinks within an imported tree are skipped by default;
- source-relative exclusion globs are persisted and reused by every scan;
- every job owns a `ScanRevision` with new/modified/moved/missing counters;
- sparse quick hashes preserve identity for an unambiguous move only when the
  prior path is absent from the current scan; identical files at extant paths
  remain distinct;
- an unavailable source and its files become `offline`, while files absent
  from an available source become `missing`;
- schema changes are recorded by `schema_migrations` rather than relying only
  on SQLAlchemy `create_all()`.

Full/perceptual hashes in the background and managed copies remain planned.
The current worker provides Tier 0 discovery, a lightweight quick-hash identity
step, and initial pairing/catalog insertion; it is not the complete priority
scheduler described in the product direction.

The current pywebview application uses the same local FastAPI UI as the browser build. Its random loopback port, per-launch session token, restricted Host header, and native API bridge are desktop concerns and do not leak into analysis engines.

## Frontend replacement plan

1. FastAPI + HTMX remains the interaction and design reference.
2. Tauri + WebGL should initially call the same versioned local application API. This validates packaging and rendering independently of engine migration.
3. egui + wgpu should use the same application-service operations and domain vocabulary. It must not read SQLite tables directly.
4. Shared acceptance scenarios—open catalog, filter, navigate, analyze, decide, and recover—must run against every promoted frontend.

## Engine replacement plan

Each engine implementation must accept an immutable analysis request and return versioned metric results with analyzer identity, implementation version, confidence, timing, and failure details. Rust candidates should be introduced analyzer-by-analyzer behind the registry rather than through a full rewrite.

A Rust engine can become the default only when it:

- passes the same unit and photographic validation corpus as Python;
- maintains or improves F1, false-rejection, and false-acceptance rates;
- shows a meaningful end-to-end speed or memory improvement on representative RAW/JPEG shoots;
- preserves cache compatibility or provides an explicit migration;
- has deterministic failure handling and a Python fallback during rollout;
- is packaged and tested on every supported desktop platform.

## Decision gates

Frontend and engine choices remain reversible until the project has representative real-world datasets. Record benchmark hardware, corpus revision, catalog size, cold/warm cache state, and build version. Promote a track only from evidence; keep experimental percentages separate from implemented readiness.

## Storage boundary

SQLite with WAL is the default transactional catalog for a local desktop application. `CatalogConfig` resolves a filesystem path, a SQLAlchemy SQLite URL, or `PHOTO_CULLER_DATABASE_URL`; PostgreSQL URLs are recognized as the first shared-server candidate. PostgreSQL is not production-ready until a driver extra, migrations, concurrency tests, backup/restore documentation, and CI are present.

DuckDB may be evaluated for read-heavy analytics and benchmark snapshots. A key/value store may be evaluated for disposable metric or thumbnail caches. Neither should replace the relational source of truth for user decisions without measured evidence and a migration plan.

The Rust workspace mirrors the same explicit `StorageBackend` vocabulary so CLI and future frontends do not hard-code SQLite access.
