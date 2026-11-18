# Derived export planning

`plan_export` validates an explicitly selected destination root and relative
output paths, queries existing destinations, and returns a plan without writing
files. A collision is recorded and never overwritten. An item with missing
W:O:A program or event context is marked for review; no minimum or quota is
imposed. `apply_plan` writes only renderer-supplied derived bytes through the
exclusive-create writer port and returns written paths, collisions, skipped
items, and session/photo/recipe provenance.

Run the standard-library-only adapter tests from the repository root:

```sh
sh rust/apps/photo-culler-server/src/editing/export/test-standalone.sh --offline
```

The base sprint does not yet provide the server crate, a durable plan-review
workflow, renderer adapter, or source for W:O:A event context. Integration must
register this module, bind the writer to an approved output root with atomic
create-new semantics, persist the report/provenance, and expose review before
apply. The adapter must also enforce that the destination root is not an
original source root; that distinction is not inferable from a plain path.
