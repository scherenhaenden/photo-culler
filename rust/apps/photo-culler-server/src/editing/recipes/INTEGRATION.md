# Recipe persistence integration status

`SqliteRecipeStore` implements the versioned recipe store against the additive
`edit_documents` table from catalog migration 6. It persists the current recipe,
undo/redo stacks, schema version and revision in one SQLite transaction. Saves
use revision compare-and-swap; a stale snapshot is returned as a typed
`RecipeStoreError::Conflict` and the domain service preserves it as
`RecipeError::Conflict`.

The store resolves opaque `photo_id` values through the catalog's `photos`
table and never opens or modifies source image files. It validates recipes and
history on both persistence and load. The history remains bounded to 50
snapshots.

The `RecipeStore` trait is synchronous. `SqliteRecipeStore` therefore requires
a supplied multi-thread Tokio runtime and callers from async handlers use
`spawn_blocking`. The loopback server mounts the HTTP DTOs and get/update/
undo/redo routes in persistent `--data-dir` mode; catalog migrations run before
the store is constructed. This is a compiled integration candidate only: no
tests or runtime acceptance are recorded, and session credentials/auth are not
integrated. Preview and export remain separate.
