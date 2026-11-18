# Cutover and rollback runbook

## Scope and gate

This runbook describes a controlled cutover from the current catalog writer to
a candidate Rust server. It does not execute a real migration. At source commit
`ad1bb99f50b0e46dd60dc2bb2b048291f064335a` (2026-11-18), the Rust server and
Slint client are not workspace packages, so production parity and the cutover
gate are not met. Keep the existing Python writer active until the integrator
has implemented and verified the real migrator, clients, and release artifacts.

The rehearsal in [`rehearse.py`](rehearse.py) creates an isolated synthetic
SQLite catalog and synthetic original files in a temporary directory. It uses
SQLite's online backup API, injects a candidate health failure, restores the
active pointer to the verified backup, and checks catalog rows and original
hashes. It never opens or changes a user's catalog or photos.

Run it with:

```sh
python3 repo/migration/cutover/rehearse.py
```

Expected report: `result=PASS`, `candidate=REJECTED`,
`writes_enabled_during_cutover=false`, `rollback_catalog=VERIFIED`, and
`originals=UNCHANGED`. A pass verifies only this synthetic procedure.

## Preflight and cutover

1. Name the release commit, application version, target platform, source and
   candidate catalog schema versions, database path, originals roots, backup
   destination, and operator. Confirm that a tested copy can be restored.
2. Put the current application into maintenance mode and stop every process
   that can write the catalog. Verify process shutdown and record the last
   committed transaction/backup time. Keep originals mounted read-only during
   migration and acceptance.
3. Create a consistent SQLite backup using SQLite's backup API or a documented
   `VACUUM INTO` procedure. Do not copy only the main `.db` file while a WAL is
   active. Preserve the original database and its WAL/SHM until the backup has
   passed `PRAGMA integrity_check`, schema checks, row counts, key/reference
   checks, decision/recipe comparison, and a restore test to a different path.
4. Migrate only the verified copy. Capture migration logs and before/after
   counts for galleries, sources, files/photos, IDs, decisions, RAW/JPEG
   links, sessions, recipes, and profile references. Resolve every mismatch;
   do not silently regenerate identities or drop unknown legacy fields.
5. Start the candidate in maintenance/read-only mode with the migrated copy.
   Check health, API contracts, permissions, logs, and clean shutdown. Exercise
   both supported clients against the same candidate and synthetic/approved
   catalog; verify the expected decisions and original hashes.
6. Switch the service's catalog pointer atomically to the candidate copy while
   writes remain disabled. Run health and read-only parity checks again. If any
   check fails, follow rollback below without accepting writes.
7. Record approval, then enable the candidate writer. Preserve the untouched
   original catalog and verified backup as rollback inputs. Keep the old
   application and launch entry available through the agreed observation
   period.

## Rollback

1. Stop candidate writes and all clients. Record the candidate catalog, logs,
   active pointer, and the time of the last candidate write before changing
   anything.
2. If no candidate writes were enabled, atomically point the service back to
   the preserved old catalog or verified backup, then start the old version in
   maintenance mode. Verify catalog integrity, IDs, decisions, recipes, and
   read-only access to originals before enabling its writer.
3. If candidate writes occurred, the old catalog is stale. Do not overwrite
   the candidate or claim a lossless rollback. Preserve both catalogs, produce
   an explicit delta of post-cutover decisions/recipes/imports, reconcile with
   the operator, and only then restore a compatible state. Never merge files
   or photos by guessed IDs.
4. Confirm health and a representative session against the restored version.
   Document which catalog became active, what happened to post-cutover writes,
   backup location and checksums, operator, time, and remaining issues.

## Hard limits

- The synthetic rehearsal is not a migrator test, schema parity proof, or
  production restore test. It has no access to the historical Python schema,
  NAS mounts, real photos, or a running candidate.
- Do not run the rehearsal's synthetic schema against a real catalog.
- A SQLite integrity check alone does not prove semantic parity or recoverable
  photos. Compare IDs, foreign keys, decisions, recipes, and original hashes.
- The legacy application may create new state. A rollback after candidate
  writes requires reconciliation; simply switching the pointer can lose those
  changes.
- No destructive deletion, in-place schema mutation, or original-file rewrite
  is part of this procedure.
