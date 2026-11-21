# Recipe editor integration

This folder contains a Slint component and Rust view-model. The application
crate exposes `crate::recipe_rest::RecipeRestClient<T>` for the persistent
recipe API. It uses the bounded `Transport` abstraction for GET, PUT, and POST,
rejects redirects, validates photo IDs before path construction, and decodes the
revision plus undo/redo capability snapshot. `409` responses are returned as
typed HTTP errors with the server code/message so the caller can reload the
authoritative recipe while retaining the local draft.

## Current boundary

`RecipeEditor` is mounted as the right-side panel in `DemoWindow` and reflects
the selected photo ID. Its load/save/undo/redo/revert callbacks are exported as
`DemoWindow` callbacks. `crate::recipe_flow::bind(&window, controller)` wires
selection changes and those callbacks to `RecipeRestClient`, including stale
selection suppression and 409 reload while retaining the current slider draft.
Both browser and desktop entrypoints now construct the controller with their
platform transport and invoke `bind`. The panel dispatches recipe API requests,
but this candidate has not been accepted against a running persistent server.

The REST adapter covers recipe load, save, undo, and redo only. Preview rendering,
export planning, and derived-file export remain separate capabilities not
exposed by the persistent recipe API; those controls stay disabled. No session
authentication is wired through either platform transport yet, so the recipe
routes must remain on the loopback-only desktop service until the shared session
flow exists.

## State and safety contract

- `EditDocument.revision` and `Recipe` are the last confirmed server state.
  `draft` is local-only until `SaveRecipe` succeeds. Revert discards the draft;
  a save conflict preserves the draft while the host reloads the current
  document for conflict resolution.
- The original preview and its hash are read-only. Edited previews and exports
  must be derivatives; recipe persistence never receives source paths.
- The export plan contains the exact destination, recipe revision, and filenames
  shown to the user. Any recipe edit invalidates that plan. Export must wait for
  confirmation of the exact destination and a matching revision/draft.
- `pending` suppresses repeated actions while a REST request is outstanding.
  The host must return responses to the Slint event loop and ignore stale UI
  callbacks after photo selection changes.
- `RecipeRestClient` uses the server routes `GET`/`PUT
  /api/v1/photos/{photo_id}/recipe` and `POST .../undo`, `POST .../redo`. The
  write carries `expected_revision` and `request_id` for atomic CAS and tracing.
- Destination-plan and derived-export endpoints remain pending on the Rust
  client/server. This panel emits `PreviewExport` and `ExportDerived` commands;
  the adapter must write only derivatives and never mutate originals.

## Remaining UI work

1. Compile and inspect desktop and browser layouts, then accept the recipe flow
   against a running persistent server.
2. Implement before/after preview rendering and derivative export as separate
   server/client capabilities; keep their controls disabled until connected.
3. Add session authentication before exposing the API beyond isolated loopback
   development.
