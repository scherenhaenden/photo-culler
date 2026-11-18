# Analysis coordinator integration notes

This is a transport-independent coordinator because the server crate, jobs
queue, and shared API DTOs are not present at the sprint base. The server
adapter should construct `Plan` from an authorized catalog plus either a
gallery or session selection, and scope all repository operations by
`catalog_id` and `job_id` (`JobKey`).

The repository must atomically replace a complete `Job` snapshot, especially
`next_index` and the matching completed photo result. `Coordinator::new`
loads persisted jobs and converts interrupted `Running` jobs to `Paused`,
discarding `partial` analyzer output. Completed per-photo results remain
committed and resume starts at the stored cursor. The analyzer receives the
catalog ID so implementations cannot accidentally share catalog state.

`submit` bounds active work by `max_running + max_queued`; `start` enforces
`max_running`. A worker calls `step` once per photo, making pause and cancel
cooperative at photo boundaries. The coordinator has one in-process scheduler;
production workers must serialize mutation of each coordinator instance and
provide durable IDs and atomic repository writes.

Suggested future shared DTO fields: job ID, catalog ID, scope kind and ID,
state, monotonic snapshot version, phase, completed/total/failed counters, and
result/error by photo. Pause reason should be explicit if manual and thermal
pauses are later combined. Routes and serialized enum names remain for the
integrator to freeze.

Standalone acceptance:

```sh
rustc --edition=2024 --test rust/apps/photo-culler-server/src/jobs/analysis/mod.rs -o /tmp/photo-culler-task25-tests
/tmp/photo-culler-task25-tests
```
