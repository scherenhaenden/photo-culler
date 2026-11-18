# Task 35: review decisions

This module ports the legacy `DecisionState` vocabulary and non-destructive
manual/bulk decision behavior behind a repository port. It requires callers to
provide an explicit session or photo-ID scope and an expected scope revision.
The repository implementation must compare the revision and persist the full
batch atomically. Only decision values cross this port; original files and
their paths are never read or written.

## Legacy contract

The preserved values are `KEEP`, `BEST`, `ALTERNATE`, `REVIEW`, `RECOVER`,
`REJECT_TECHNICAL`, `REJECT_REDUNDANT`, `PROTECTED_BY_COVERAGE`, and
`UNPROCESSED`, matching `photo_culler/core/enums.py`. The legacy web service
maps `reject` to `REJECT_TECHNICAL`; the CLI accepts explicit enum names.
This module exposes the enum and stable strings but leaves HTTP aliases and
error DTO mapping to the future API integration.

## Integration requirements

1. Register the module from the server's review aggregator when it is added.
2. Implement `DecisionRepository` with the catalog's transaction owner. Its
   `apply_atomically` must compare-and-set the scope revision, check membership
   again in the same transaction, apply every decision or none, and increment
   the revision once. A SQL implementation should update decision columns only.
3. Map revision mismatch to the common HTTP conflict response and missing IDs
   to the agreed not-found/validation response. Do not silently map unknown
   state strings to `REVIEW`; parse them at the API boundary as validation
   errors.
4. Add adapter tests against the actual catalog transaction and verify a
   rejected decision can be corrected after reload without modifying originals.

## Standalone acceptance tests

The server crate and shared Cargo test harness are not present at base
`eda2b334`; run the module's self-contained tests with:

```sh
rustc --edition=2021 --test rust/apps/photo-culler-server/src/review/decisions/mod.rs -o /tmp/photo-culler-decisions-tests
/tmp/photo-culler-decisions-tests
```
