# Import worker integration notes

This base does not yet contain `photo-culler-server`, a shared import harness, a
catalog repository, or the task 14/15 job API. The worker is therefore delivered
as a dependency-injected Rust module with standalone unit tests.

The integrator still needs to:

1. Register `worker` from the future `import` module and add the server crate to
   the workspace in the shared integration change.
2. Implement `Scanner` over the authorized source roots. It must return stable,
   source-relative paths in strictly increasing lexical order, honor the exclusive
   cursor, and only read originals.
3. Implement `ImportRepository` over the catalog/job store. In one transaction,
   `commit_observation` must classify new/modified/moved/unchanged entries and
   persist the cursor and job progress. Path/hash matching for unambiguous moves
   belongs with task 23 identity logic; an identical file still present at another
   path must remain a separate catalog entry.
4. Make `finish_job` idempotently mark entries in the job's source missing when
   absent from the complete scan. Persist enough per-job seen-path state to support
   this when the job resumes after a process restart.
5. Connect queued/running/paused/completed states to the shared job snapshot API.
   Startup must call `recover_interrupted`; the operator/UI must call `resume`
   explicitly for paused work. A yielded running job is not a completed scan.

`commit_observation` must make the catalog update and cursor checkpoint atomic.
`finish_job` must also be idempotent because a crash can happen after its catalog
effects but before the worker records `Completed`. Neither method should write to
or rename original files; all writes belong to the catalog and derived-output
areas.

Standalone acceptance command from the repository root:

```sh
rustc --edition=2021 --test rust/apps/photo-culler-server/src/import/worker/mod.rs \
  -o /private/tmp/photo-culler-task24-worker-tests
/private/tmp/photo-culler-task24-worker-tests
```
