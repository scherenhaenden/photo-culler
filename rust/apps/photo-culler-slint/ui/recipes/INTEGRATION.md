# Recipe editor integration

This folder contains an isolated Slint component and a Rust view-model with an
injected REST command/response boundary. At base `dfb7434`, the repository has a
shared `RestClient` and Slint component library. `RestClient` currently exposes
gallery-page GET only; it has no recipe read/write, preview, destination-plan,
or export methods. The `photo-culler-slint` application root, its crate manifest,
and the root registration for UI panels are still pending. The integrator must
register `RecipeEditor` and wire its callbacks to `RecipeEditor` commands.

## State and safety contract

- `EditDocument.revision` and `Recipe` are the last confirmed server state.
  `draft` is local-only until `SaveRecipe` succeeds. Revert discards the draft;
  a save conflict preserves it and exposes the confirmed conflict snapshot.
- Both preview panes are image URLs supplied by the host. The original preview
  and its hash are read-only; edited previews and exports are derivatives.
- The export plan contains the exact destination, recipe revision, and filenames
  shown to the user. Any recipe edit invalidates that plan. Export is rejected
  until the user confirms the exact destination and the plan still matches the
  current revision/draft.
- `pending` suppresses repeated save/export/preview actions while a REST request
  is outstanding. Responses must match the outstanding action.
- The Rust client still needs typed recipe read/write and preview endpoints.
  Legacy reference routes exist at `GET /api/v1/photos/{photo_id}/edit`,
  `PATCH /api/v1/photos/{photo_id}/edit`, and
  `GET /api/v1/photos/{photo_id}/edit-preview?max_size=...`. The Rust edit write
  should carry `expected_revision`; map stale writes to
  `RecipeResponse::Conflict`.
- Destination-plan and derived-export endpoints also remain pending on the Rust
  client/server. This panel emits `PreviewExport` and `ExportDerived` commands;
  the adapter must write only derivatives and must never mutate original files.
- Dispatch REST commands off the UI thread. The current client transport is
  synchronous; run commands on a worker/executor and return responses to the
  Slint event loop so loading or network latency cannot block input and redraw.

## Standalone tests

The Slint application root and recipe REST endpoints are not present, so the
Rust controller's unit tests run against a REST simulator directly:

```sh
rustc --edition=2024 -D warnings --test rust/apps/photo-culler-slint/ui/recipes/mod.rs \
  -o /private/tmp/photo-culler-task51-recipes-tests
/private/tmp/photo-culler-task51-recipes-tests
```

The `.slint` component cannot be compiled until the Slint app root and its
pinned dependency registration are integrated. This area intentionally does
not modify manifests or root modules.
