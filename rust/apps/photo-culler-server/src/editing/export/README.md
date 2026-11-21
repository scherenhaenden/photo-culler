# Derived export planning

`plan_export` validates an explicitly selected absolute destination root and
relative output paths, queries existing destinations, and returns a plan
without writing files. A collision is recorded and never overwritten. An item
with missing W:O:A program or event context is marked for review; no minimum or
quota is imposed. `apply_plan` writes only renderer-supplied derived bytes
through the exclusive-create writer port and returns written paths, collisions,
skipped items, and session/photo/recipe provenance.

`LocalDerivedWriter` is the filesystem adapter for a destination root that
already exists. It canonicalizes the root and configured source roots, refuses
overlap between them, rejects symlink traversal under the export root, creates
subdirectories beneath that root, and uses `create_new` plus `sync_all` for each
derived file. The application must construct it from trusted configuration; the
HTTP contract uses opaque destination IDs and never accepts local filesystem
paths or rendered bytes from a caller.

Run the standard-library-only adapter tests from the repository root:

```sh
sh rust/apps/photo-culler-server/src/editing/export/test-standalone.sh --offline
```

The `http::exports` module defines plan, review, and apply routes, but neither it
nor this filesystem writer is mounted in the persistent server runtime yet.
There is no render adapter, durable plan store, trusted export-destination
registry, or source of W:O:A event context. Do not report exports as ready.
Before wiring the routes, integrate the renderer and catalog snapshot, persist
the plan with a digest/revision, and make apply idempotent. The adapter's source
root exclusion is necessary but not a substitute for session authorization.

The path checks reduce symlink escape risk, but standard-library path checks
cannot prevent a same-user process from renaming a checked parent and placing a
symlink or reparse point before file creation. `symlink_metadata`/canonicalize
and a later path-based `OpenOptions::open` are separate operations, so adding
another check or retry would not close the race.

No partial mitigation is implemented. The server currently has no direct
capability-filesystem dependency: `rustix` and `windows-sys` in the lockfile
come from transitive dependencies, and `tempfile` is dev-only. They cannot be
used as the product's portable filesystem contract. Closing this gap needs a
root-handle-anchored writer that walks/creates each parent relative to an open
directory handle without following links/reparse points, then creates the leaf
exclusively relative to the verified parent. The implementation must provide
and document equivalent guarantees on macOS/Linux and Windows, and be race-tested
for parent rename/link replacement before this writer is treated as a security
boundary. Until then, do not mount HTTP export or treat this adapter as safe
against a hostile concurrent local process.
