# D25 analysis integration status

This candidate registers the analysis coordinator, durable SQLite repository,
catalog-to-plan resolver, Rust pixel-engine adapter, HTTP contract and SQLite
API owner. The persistent runtime mounts its routes and ingress allowlist when
authorized roots are configured. The Processing control is wired on desktop
and browser to discover an existing queued/running/paused job for the selected
gallery before submitting. It reconnects to that job after application restart,
starts a recovered queued job, and lets the user resume a recovered paused job.
The flow also polls and exposes pause/resume/cancel controls. A limited API smoke completed analysis for
two synthetic JPEGs (2/2, zero failures); manual loopback runtime acceptance
also confirmed that Start rejects a queued job and Resume rejects a recovered
paused job after reimport changes the plan. These checks do not cover the UI or
the wider D25 acceptance scope. See the
[analysis plan revalidation evidence](../../../../../../docs/runtime-evidence/2026-11-21-analysis-plan-revalidation-smoke.md)
and the earlier
[synthetic persistent smoke](../../../../../../docs/runtime-evidence/2026-11-21-synthetic-persistent-smoke.md).

## Persistence and recovery

- `SqliteRepository::initialize` creates the additive
  `photo_culler_analysis_jobs` table using the catalog's single-writer pool.
- Each `Coordinator` snapshot is encoded as JSON and upserted with its state,
  cursor (`next_index`), and timestamps. A single SQL transaction persists the
  snapshot and writes the last completed photo's metrics under
  `photos.analysis_summary_json.rust_shadow`; previous summary keys are kept.
- Startup checks that the indexed catalog/job/state/cursor columns agree with
  the JSON snapshot, that `completed` is the exact plan prefix at `next_index`,
  photo IDs are unique, and terminal/queued cursors are consistent. An invalid
  snapshot fails coordinator recovery rather than silently skipping or
  re-running a photo.
- `Coordinator::new` reloads snapshots and converts a persisted `Running` job
  to `Paused`, discarding uncommitted partial output. Callers must make
  coordinator operations from a blocking context when using the synchronous
  trait adapter; the supplied Tokio runtime must be multi-threaded.
- Every `start` or `resume` re-resolves the queued/paused plan against the
  catalog and the server's currently configured authorized roots. The job may
  resume only when its catalog, gallery scope, profile, and ordered photo IDs
  still match the persisted plan; fresh resolved source paths replace stale
  ones before the state changes to `Running`. If catalog membership or order
  changed, the request is rejected as an invalid plan and the operator must
  rescan/reimport and submit a new job. This prevents a recovered cursor from
  silently advancing over a changed catalog or reading a path outside the
  current roots.
- `prepare_gallery_plan` reads present files for one gallery and resolves a
  path only when it remains inside a caller-supplied authorized root. It
  rejects symlink components and does not return paths through an HTTP DTO.

## State transitions

`Queued → Running → Succeeded` is the normal path. A running job can become
`Paused` or `Cancelled`; a process restart recovers `Running` as `Paused`, and
the caller must explicitly resume it. Per-photo analyzer errors are retained
as failed outcomes in an otherwise completed job and contribute to its failed
count.

## HTTP control plane

The endpoints and sanitized progress DTO are documented in
[`HTTP_INTEGRATION.md`](HTTP_INTEGRATION.md). Submit resolves the gallery plan
from configured roots; start/resume launches an in-process worker; pause and
cancel take effect between photos. The API projection does not expose local
paths or raw analyzer errors. Its router is deliberately a separate module so
the runtime can mount it only on the loopback persistent-catalog server.

## Boundaries still open

- The desktop/browser Processing flow polls queued/running jobs every two
  seconds, retains the job ID after transient errors, and exposes
  pause/resume/cancel. The active-job lookup reconnects one persisted job per
  gallery after application restart. The flow still lacks integrated UI
  runtime/visual acceptance, offline backoff, and explicit handling for
  multiple active jobs in the same gallery.
- Plan creation has no session scope adapter. It selects the preferred present
  JPEG/image/RAW path per photo, but the current pixel engine decodes raster
  formats only; RAW files produce a visible per-photo failure until embedded
  preview extraction is wired.
- Rust pixel results are saved under `rust_shadow`. They do not update the
  production score, quality tier, decision, or legacy-selected Python result.
  The profile identifier is recorded, but calibrated profile scoring and
  shadow comparison are not implemented here.
- Session authentication now protects the API and has a limited loopback smoke;
  analysis UI reauthorization and runtime acceptance are not complete.
  Persistent worker supervision, benchmarks, and five-target runtime
  acceptance remain open and must be recorded against one candidate SHA.
- Fresh-plan-before-start/resume behavior has narrow manual loopback acceptance
  for changed-plan rejection after reimport, including a recovered paused job.
  A process restart converts running work to paused; it does not automatically
  restart analysis, and each photo is still the only pause/cancel boundary.

Do not treat the presence of this adapter, the minimal Processing flow, or the
two-photo smoke as completed D25 acceptance. Runtime verification on the
integrated UI and a wider authorized corpus is still required.
