# Recipe API contract

The router in `../recipes.rs` exposes versioned, non-destructive recipe state
for one catalog photo. It accepts an opaque `photo_id`; no filesystem path is
accepted or returned. Recipe updates never write image originals.

## Routes

- `GET /api/v1/photos/{photo_id}/recipe` returns the current recipe snapshot.
- `PUT /api/v1/photos/{photo_id}/recipe` replaces the current recipe only when
  `expected_revision` matches the stored document revision.
- `POST /api/v1/photos/{photo_id}/recipe/undo` and `/redo` apply one history
  transition after the same revision check.

Writes require a client-generated `request_id` (trimmed, nonempty, at most 128
bytes) and an unsigned `expected_revision`. PUT JSON:

```json
{
  "expected_revision": 3,
  "request_id": "edit-4d3a",
  "recipe": {
    "schema_version": 1,
    "exposure": 0.25,
    "temperature": 6100.0,
    "tint": 2.0
  }
}
```

Undo/redo JSON:

```json
{"expected_revision": 4, "request_id": "undo-4d3a"}
```

Success returns `contract_version`, `request_id`, `photo_id`, `revision`, the
current recipe values, and `can_undo`/`can_redo`. Errors use
`contract_version`, `code`, `message`, and `request_id`; revision conflicts
also include `actual_revision`. The request ID is repeated in the
`X-Request-Id` response header.

| Status | Error code | Meaning |
|---|---|---|
| 400 | `invalid_request` | Invalid photo ID or request envelope. |
| 400 | `invalid_recipe` | Unsupported schema or value outside its domain range. |
| 404 | `not_found` | Photo is absent from the catalog. |
| 409 | `version_conflict` | Another write advanced the recipe revision. |
| 409 | `history_unavailable` | Undo or redo has no matching history item. |
| 503 | `catalog_unavailable` | Persistent catalog operation failed. |

The router is mounted in persistent `--data-dir` server mode. The server binds
only to loopback and ingress applies exact route/method and Host/Origin checks.
Those checks do not authenticate local processes: session credentials/store
are not integrated, so do not expose this candidate over LAN. Export and
preview remain separate features; this API does not render, copy or export
files. No runtime acceptance is claimed.
