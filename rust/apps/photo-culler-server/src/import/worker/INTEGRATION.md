# Import worker integration status

`photo-culler-server` registers the persistent worker in `import::worker` and
uses `SqliteImportRepository` with the catalog's single-writer pool. The server
API and both Slint clients are wired to this worker. This note describes the
current implementation boundary; it is not runtime acceptance evidence.

## Data flow

1. A job and scan revision are created for a registered source.
2. `Scanner::build_manifest` walks the configured root and sends bounded,
   ordered batches. The repository persists portable source-relative paths and
   metadata in `photo_culler_import_manifest_entries`.
3. After the manifest is complete, the worker reads ordered pages after its
   durable cursor. `Scanner::observe_page` reopens and hashes each path; the
   repository commits observations, cursor, and processed count atomically.
4. `finish_job` stages observations and existing source rows into temporary
   SQLite tables, validates pairing/move ambiguity, reconciles catalog rows,
   marks unseen files missing only when their old paths fall inside the proven
   scan coverage, records the result, and clears successful or cancelled
   staging in one catalog transaction. Paths affected by configured
   exclusions, non-recursive subdirectories, skipped paths, or uncertain
   filesystem errors are kept out of missing classification and move matching.
   When the API marks a job
   failed, it also clears its non-resumable observation and manifest staging
   transactionally while retaining the failure code and server log.
5. On restart, an incomplete manifest is rebuilt; a complete manifest resumes
   from the saved cursor. Recovered jobs remain paused until an explicit resume.

The scanner reads originals only. It validates source-relative paths, checks
that roots and opened files remain under locally configured authorized roots,
and rejects size/mtime changes between manifest creation and hashing. It does
not rename or write original files.

Failure-state persistence uses up to three process-level attempts, separated
by 50 ms and 100 ms delays. Each attempt uses a fresh transaction to update the
job and revision and clear staging atomically. If all attempts fail, the API
logs that failure and returns; it does not keep an unbounded retry queue or
claim the job was marked failed. The transaction rolls back on update or
cleanup errors, so the job and staging remain unchanged together. The job may
remain `queued` or `running` until restart recovery changes it to `paused`; the
operator can then explicitly resume it. A cancellation accepted before the
failure transaction takes precedence and is persisted as cancellation instead.
If `COMMIT` returns an error, the next attempt checks the persisted job state
before applying another transition; an already committed `failed` or
`cancelled` state is treated as success because state and cleanup commit in one
transaction. If SQLite remains unavailable through all attempts, the final
commit outcome may still be unknown until the database can be inspected.

`commit_observation` does not mutate catalog file rows. It stores observations
for the active job and returns a preliminary `EntryChange`; final new/modified/
moved/unchanged counts come from catalog reconciliation in `finish_job`. Do not
use per-observation change values as a durable scan result.

## Remaining integration and acceptance work

- Pairing behavior is implemented separately in `import::pairing`; the SQLite
  finalizer currently groups by its own `logical_key` rules. Consolidate or
  demonstrate parity before changing supported file types or ambiguous-pair
  behavior.
- A small synthetic runtime smoke now covers one unambiguous move, an identical
  copy, reuse of the moved-from path with different content, an excluded partial
  scan, restart persistence and a repeated full scan. Evidence is recorded in
  `docs/runtime-evidence/2026-11-21-import-reconciliation-smoke.md`. Skipped
  symlinks, ambiguous moves and real-catalog behavior still need acceptance.
- A second loopback runtime smoke exercised restart recovery from a completed
  manifest with 100 of 12,000 observations committed. SIGKILL of the server
  process left the job persisted as `running`; startup changed it to `paused`
  at the same cursor, and explicit resume completed with 12,000 file rows and
  12,000 distinct photo IDs. Evidence is recorded in
  `docs/runtime-evidence/2026-11-21-import-crash-resume-smoke.md`. A crash
  during partial manifest construction and UI reconnection are not covered.
- The temporary-table finalizer pages normal observation and existing-file
  rows, while legacy absolute-path adoption is also staged in a temporary table.
  The 10,000-entry benchmark and peak-RAM measurement have not been run.
- Deleting staging makes SQLite pages reusable but does not shrink the database
  file; the candidate does not run `VACUUM` automatically.
- API/desktop/browser controls, operator pause/resume/cancel, interrupted-job
  restart recovery from a **partial** manifest, authorized-root rejection and
  real-photo behavior still need runtime acceptance against one recorded
  commit SHA. Follow the separate procedure in
  `docs/rust-slint-demo/RUN.md` and record each surface independently.

The server passed `cargo build --offline --locked` for the crash/restart smoke;
the integrated server/CLI/Slint native and Slint WASM checks also passed after
the D25 plan-validation and UI-resynchronization changes. No test suites or
cross-platform CI were run for this candidate; runtime acceptance is limited to
the cases explicitly linked above. Keep implementation evidence, runtime
acceptance, and product-level migration completion as separate claims.

- A third loopback runtime smoke killed the server only after SQLite showed a
  partial manifest (`building`, 256 entries) and a running job. Restart recovered
  the job as paused; explicit resume completed with 20,000 processed/imported
  items and 20,000 file and photo rows. Evidence and limits are recorded in
  `docs/runtime-evidence/2026-11-21-import-partial-manifest-crash-resume-smoke.md`.
  Distinct photo IDs were not independently queried in this run.
