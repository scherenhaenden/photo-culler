# Import partial-manifest crash/restart/resume runtime smoke

**Date:** 2026-11-21 (UTC)
**Candidate:** worktree `26991eb`
**Host:** Darwin arm64; server bound to an ephemeral `127.0.0.1` port only
**Server binary:** shared `/private/tmp/photo-culler-d25-smoke-target/debug/photo-culler-server`
**Binary SHA-256:** `9068852e88c7ed4432f1595b3b860b96ebbaf552b25abaa8b1c7b1d4e3b6a4de`

## Fixture and run

The authorized root was an isolated temporary directory under
`/private/tmp/d24-partial-manifest-smoke/authorized`. It contained 20,000
independent copies of `migration/demo/fixture/demo-001.jpg`, each 40,001 bytes
(800,020,000 bytes total). The source fixture SHA-256 was
`f996f797fde19c10187d19538ad738a18235c6a9b03d34b85b896cff61401922`; the
repository source was read only. Catalog data was isolated under the same
`/private/tmp` directory.

Started a persistent owned server and launched a recursive import through the
authenticated loopback API. Polled the live SQLite catalog read-only. The first
captured partial checkpoint was manifest `building`, 256 manifest entries, and
job `running` with zero discovered/imported observations. Only then sent
SIGKILL to the candidate server PID (`38468`); its exit code was `-9`, and the
database still showed that same partial-manifest checkpoint.

Restarted the server against the same catalog. The job API reported `paused`
with zero observations processed. Explicit resume returned HTTP 202. The job
completed with `processed=20000`, `imported=20000`, `issues=0`; the final
catalog contained 20,000 `files` rows and 20,000 `photos` rows. The manifest
staging rows were gone after successful completion. This supports recovery and
no count-level duplication for this fixture; distinct photo IDs were not
independently queried in this run.

The server shut down cleanly. The fixture, catalog, and temporary scripts were
deleted. `/private/tmp` had 57 GiB free afterward, unchanged from the pre-run
rounded reading. The shared build target was retained.

## Scope and limits

This is one manual runtime acceptance of interrupted **partial manifest
construction** followed by restart and explicit resume. It confirms recovery
through the server API and final record counts for this synthetic fixture. It
does not establish behavior for real photo catalogs, cross-platform execution,
repeated crash cycles, UI reconnection, or large catalog performance. No build
or test suite was run by this smoke agent.

## Static audit: identity, replay, and transaction boundaries

The source review supports a **schema-backed uniqueness conclusion for the
fresh isolated catalog used by this run**, with a separate caveat for legacy
catalogs:

- The fresh SQLite schema declares `photos.photo_id` `UNIQUE NOT NULL` and
  `files` unique on `(photo_id, relative_path)` ([catalog schema](../../rust/apps/photo-culler-server/src/catalog/sqlite/mod.rs:71)).
  The smoke used a unique, new temporary data directory; `Catalog::open_initialized`
  creates this baseline when no catalog tables exist
  ([catalog initialization](../../rust/apps/photo-culler-server/src/catalog/sqlite/mod.rs:107)).
  Under that exact fresh-schema path, the reported 20,000 rows in `photos`
  necessarily have 20,000 distinct `photo_id` values because SQLite enforces
  the column constraint. The smoke did not independently run a
  `COUNT(DISTINCT photo_id)` query or inspect `index_list` on the live database;
  this conclusion follows from the executed source path and schema, not from a
  direct uniqueness query in the captured output.
  The smoke binary predated migration v9, so it did not add/verify a missing
  legacy unique index. A subsequent candidate change adds that index inside the
  migration transaction when no full, single-column unique index exists; any
  duplicate IDs or null/empty IDs make the migration fail and roll back. The
  legacy validation path checks columns before the migration; the new migration
  validates IDs and handles the index after that validation
  ([legacy validation](../../rust/apps/photo-culler-server/src/catalog/sqlite/mod.rs:166),
  [migration v9](../../rust/apps/photo-culler-server/src/catalog/migration/mod.rs:132),
  [legacy-schema migration fixture](../../rust/apps/photo-culler-server/src/catalog/migration/mod.rs:293)).
- After a crash with an incomplete manifest, resume runs `reset_manifest` in
  a transaction: it deletes partial entries and staged observations, resets
  the cursor/counts, and sets the manifest back to `building`. The worker then
  rebuilds the manifest before hashing any page
  ([reset](../../rust/apps/photo-culler-server/src/import/repository.rs:442),
  [resume flow](../../rust/apps/photo-culler-server/src/import/worker/mod.rs:373)).
  Thus the 256 old entries are discarded, not appended to a fresh traversal.
- The staging keys are `(job_id, path)` for manifest entries and observations.
  Replaying a committed observation with different size/hash fails; replaying
  the same path/hash does not increment the cursor/count a second time. The
  observation row and checkpoint/count updates commit together
  ([observation staging table](../../rust/apps/photo-culler-server/src/import/repository.rs:167),
  [manifest staging table](../../rust/apps/photo-culler-server/src/import/repository.rs:201),
  [observation transaction](../../rust/apps/photo-culler-server/src/import/repository.rs:671)).
- During finalization, stable IDs derive from gallery, source path, and
  normalized logical photo key; a reused-path fallback also includes the full
  content hash. Before catalog upsert, `validate_finish_group_identities`
  rejects two logical groups resolving to the same photo ID. `photos.photo_id`
  uniqueness is the final database guard when that constraint is present
  ([identity assignment](../../rust/apps/photo-culler-server/src/import/repository.rs:1300),
  [duplicate-group check](../../rust/apps/photo-culler-server/src/import/repository.rs:1425),
  [upsert](../../rust/apps/photo-culler-server/src/import/repository.rs:1445),
  [identity derivation](../../rust/apps/photo-culler-server/src/import/repository.rs:2144)).
  The temporary group table itself has a primary key on `logical_key`, not on
  `photo_id`; the explicit validation is material.
- `finish_job` applies group assignment, photo/file writes, missing-file
  updates, terminal job/revision state, and staging cleanup inside one SQLite
  transaction. A process crash before commit should roll those database
  writes back together; this audit did not kill the process inside that
  transaction. `claim_job` uses a state-conditional update to ensure a queued
  or paused job is claimed once per supported worker/database instance
  ([finalization](../../rust/apps/photo-culler-server/src/import/repository.rs:817),
  [claim](../../rust/apps/photo-culler-server/src/import/repository.rs:294)).

The smoke did **not** query `COUNT(DISTINCT photos.photo_id)` or inspect
foreign-key/path associations. It did not crash during `reset_manifest`,
observation commit, `complete_manifest`, or finalization, or run two
workers/processes against the same database. `Catalog` documents one writer
instance per catalog and uses a single writer connection per instance;
cross-process concurrent writers are outside that stated topology
([writer setup](../../rust/apps/photo-culler-server/src/catalog/sqlite/mod.rs:325)).
The finish transaction plus `validate_finish_group_identities` makes duplicate
new group identities a failure rather than silently merging them; successful
finish on the fresh schema, plus 20,000 photo rows, therefore supports the
distinct-ID conclusion for this run. It is still not evidence for unsupported
legacy schemas, other crash points, or concurrent processes.

Filesystem observations check file metadata before and after hashing, but do
not lock the whole source tree through finalization; a later mutation after a
file's observation was not exercised here.
