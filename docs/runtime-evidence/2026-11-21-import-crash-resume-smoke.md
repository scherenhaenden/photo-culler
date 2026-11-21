# Import crash/restart/resume runtime smoke

**Date:** 2026-11-21 (UTC)
**Candidate:** uncommitted worktree based on `26991eb`
**Host:** Darwin arm64; server bound to an ephemeral loopback port only
**Server build:** `cargo build --offline --locked --manifest-path rust/Cargo.toml -p photo-culler-server`
**Temporary server binary SHA-256:** `885bef71df06da3271592506aa910260c4c8a899fdc88761f1b95fb0381043d9`

## Fixture and run

The authorized source root was created under
`/private/tmp/photo-culler-import-recovery-runtime/authorized/input`. It held
12,000 independent file copies of the repository's synthetic JPEG fixture,
40,001 bytes each. The fixture SHA-256 was
`f996f797fde19c10187d19538ad738a18235c6a9b03d34b85b896cff61401922`; the
repository fixture was read only. The catalog and source files were isolated
under `/private/tmp`.

Started an owned persistent server with a fresh SQLite catalog, started one
recursive import through the authenticated loopback API, and observed the
SQLite checkpoint after the manifest reached `complete` and 100 observations
had committed. Sent SIGKILL only to that server process. On restart against the
same catalog, the job was `paused` at the same cursor (100). Explicit resume
completed the job with `processed=12000`, `imported=12000`, 12,000 catalog file
rows and 12,000 distinct photo IDs. The identical-content copies remained
independent records; the resume did not replay or duplicate the first 100
observations.

The server then shut down cleanly. The fixture, database, server binary and
Cargo target were deleted after capture.

## Scope

This is limited runtime acceptance of the **complete-manifest / partial-hash
cursor** recovery path. It does not exercise a crash during partial manifest
construction, an operator pause/cancel racing with process death, inaccessible
source roots after restart, UI reconnection, large real catalogs, or
cross-platform behavior. No test suite was run.
