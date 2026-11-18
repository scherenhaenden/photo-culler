# Task 34: session-scoped review and hybrid grouping

This standalone module ports the legacy timeline/hybrid grouping behavior into a
storage-independent domain port. The current base has no server crate root,
grouping core module, or Rust catalog schema to register it with.

`ReviewRepository` must be implemented by the catalog owner. Its
`replace_review_state` operation must replace the generated sessions and groups
atomically. Persist `session_id`, `group_id`, the explicit `group.session_id`,
member IDs and representative ID. Session and group IDs are deterministic for
the same timestamp span and ordered member identities. A changed membership
produces a new ID; renaming a session belongs to the session record and must not
change its identity.

The repository adapter should map legacy `PhotoDB.session_id` and
`PhotoDB.burst_id` only after migration defines their stable-ID constraints.
Burst groups are generated inside each session, never across session boundaries.
Undated photos remain unassigned. The current legacy implementation uses random
UUID IDs and supports timeline, burst and hybrid profile operations; this module
provides the hybrid review domain only. The HTTP/router owner should expose the
filter fields and require both session and group identifiers when changing a
representative. Authentication and revision conflict handling belong to their
respective server components.

Acceptance test (no Cargo dependency):

```sh
rustc --edition=2021 --test rust/apps/photo-culler-server/src/review/sessions/mod.rs -o /tmp/photo-culler-task34-tests
/tmp/photo-culler-task34-tests
```
