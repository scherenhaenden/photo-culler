# Derived export HTTP contract

`http::exports::router()` is a contract module only; it has no concrete
`ExportApi` implementation and is not mounted in `demo/runtime.rs`.

## Routes

| Method and path | Behavior |
| --- | --- |
| `POST /api/v1/galleries/{gallery_id}/export-plans` | Build a plan from `{session_id, destination_id, photo_ids}`. The destination is an opaque trusted configuration ID. |
| `GET /api/v1/export-plans/{plan_id}` | Read the review snapshot: relative output names, disposition and review reasons, plus plan revision/digest. |
| `POST /api/v1/export-plans/{plan_id}/apply` | Apply only after `{expected_plan_revision, confirmed_plan_digest, request_id}` match the persisted plan the user reviewed. |

The DTOs do not contain source paths, absolute destination paths or image
bytes. `photo_ids` are bounded to 100 unique catalog IDs. `output_name` is a
relative destination name, not a local path. Apply returns per-photo output
names, collisions and skipped-review reasons, not the filesystem root.

## Required implementation before mounting

- Resolve `destination_id` only from trusted server configuration; never accept
  a path from the request.
- Canonicalize the approved output root and all configured source roots and
  reject equal or nested roots. Use `LocalDerivedWriter` for exclusive
  create-new writes and refuse symlink traversal.
- Read the selected session/catalog snapshot and recipe revisions, render
  derivatives on the server, and keep original bytes/paths outside HTTP DTOs.
- Persist the full plan and digest/revision. On apply, compare both, recheck
  collisions, and make the request ID idempotent. A stale plan returns 409.
- Do not apply items in `collision` or `needs_review` state. Missing W:O:A
  program/event context must remain visible for review; it must not be
  auto-filled or treated as approval.
- Put the apply route behind the shared edit authorization contract. Loopback
  and Origin checks do not authenticate the user.

The current server has no renderer adapter, durable export-plan store,
destination registry or W:O:A context source. This contract is not product
acceptance, and exporting remains unavailable until those pieces are wired and
verified against an isolated destination while original-file hashes stay
unchanged.

## Runtime integration audit (2026-11-21)

The current runtime boundary was checked before deciding whether these routes
can safely be mounted:

- `demo/runtime.rs::build_catalog_router` and
  `demo/runtime.rs::build_import_catalog_router` accept catalog, recipe,
  analysis and preview services. Neither constructs an `ExportApi` nor merges
  `http::exports::router()`; the fixture router also has no export service.
- `demo_args.rs` and `main.rs` configure the catalog/cache directory, gallery,
  import-only authorized roots and static web bundle. There is no trusted
  destination ID-to-root configuration. Treating `--data-dir` or an import
  root as an export destination would silently expand write authority.
- `editing/export::ExportItem` requires already-rendered derived bytes and
  `WoAContext`; `apply_plan` is an in-memory operation. The recipe SQLite API
  stores recipe/history state, but does not produce rendered output or a durable
  export plan. The preview renderer outputs JPEG previews, not an edited,
  confirmed derivative.
- `http/session/runtime.rs` classifies unknown paths fail-closed. If the
  contract router were merged without a concrete implementation it would add
  endpoints that can only return unavailable; it would not complete export.
- `LocalDerivedWriter` uses canonical-path and symlink checks plus exclusive
  file creation, but its documentation explicitly retains a same-user
  parent-directory TOCTOU race. It is not enough to expose a hostile-local-user
  HTTP write boundary.

**Decision:** leave export routes unmounted. Mounting a stub would create the
appearance of a supported export flow while no destination authority, render
semantics, durable review snapshot, replay-safe apply, or output provenance
exists. This is a source audit, not runtime acceptance. The next safe increment
is a trusted destination registry plus immutable plan persistence/digest and
idempotency contract; the renderer must then produce a derivative from the
reviewed recipe revision before a restricted apply route is exposed.

## Cross-platform filesystem race decision

`LocalDerivedWriter` still has a same-user parent-directory TOCTOU race. Its
standard-library checks resolve/inspect paths, then `OpenOptions::open` later
resolves the path again. Another process can rename a checked parent and replace
it with a symlink or Windows reparse point between those calls. Rechecking the
path narrows timing but cannot bind validation to the create operation.

No partial code defense was added: the server has no direct capability
filesystem dependency, and the `rustix`/`windows-sys` packages in `Cargo.lock`
are transitive (with `tempfile` only a dev dependency). A portable fix must
anchor traversal and creation to an open root/parent handle and refuse links or
reparse points at each component on macOS/Linux and Windows. Select a direct
cross-platform capability API or implement and separately review native
handle-relative adapters for all supported platforms; verify replacement races
before mounting the apply route. Until that work is done, keep the HTTP export
routes unmounted and do not claim the writer is safe against a hostile local
concurrent process.
