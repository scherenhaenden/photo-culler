# D25 HTTP integration contract

`http::analysis::router()` is the transport contract for durable gallery
analysis. `http::analysis_sqlite::SqliteAnalysisApi::initialize` creates or
recovers the SQLite coordinator. The application supplies its configured
authorized roots; HTTP callers cannot provide filesystem paths.

## Routes

| Method and path | Behavior |
| --- | --- |
| `GET /api/v1/galleries/{gallery_id}/analysis-jobs` | Return `{ "contract_version": 1, "active_job": <job-or-null> }` for reconnecting the UI. The server returns the most recently updated queued, running, or paused job in that gallery; terminal jobs are excluded. |
| `POST /api/v1/galleries/{gallery_id}/analysis-jobs` | Submit a job with `{ "job_id": "...", "profile_id": "technical" }`. The server resolves present files inside configured authorized roots. The returned state is `queued`. |
| `GET /api/v1/analysis-jobs/{catalog_id}/{job_id}` | Read aggregate state and progress. |
| `POST /api/v1/analysis-jobs/{catalog_id}/{job_id}/start` | Start a queued job and launch the in-process worker. |
| `POST /api/v1/analysis-jobs/{catalog_id}/{job_id}/pause` | Request pause at the next photo boundary. |
| `POST /api/v1/analysis-jobs/{catalog_id}/{job_id}/resume` | Resume a paused job and relaunch its worker. |
| `POST /api/v1/analysis-jobs/{catalog_id}/{job_id}/cancel` | Cancel at the next photo boundary. |

The status response includes contract version, catalog/gallery/job/profile
identifiers, state, completed/total/failed counts, percentage and timestamps.
It does not include resolved paths, raw analyzer errors or metrics. Rust
metrics are persisted in the existing `analysis_summary_json.rust_shadow`
field by the repository adapter.

The active-job lookup uses the same sanitized job projection and the same
Editor-authenticated, loopback-only route group as submission and control. It
does not accept a filesystem path or return resolved source paths. When several
active jobs exist for a gallery, the lookup returns the one with the latest
`updated_at` (then `created_at` and job ID as deterministic tie-breakers); the
current Processing UI tracks one job per gallery.

## Operational boundaries

- Every coordinator operation and image decode runs on Tokio's blocking pool;
  HTTP executor threads do not perform synchronous image work.
- One photo is the pause/cancel boundary. A current decode finishes before a
  pause or cancellation is observed.
- Running jobs recover as paused after restart. The user or UI must resume
  them explicitly. Before `start` or `resume`, the server rebuilds the plan
  using the current catalog and authorized roots. It requires the persisted
  catalog, scope, profile and ordered photo IDs to match, then replaces source
  paths with freshly resolved paths. A changed photo set is rejected with
  `invalid_plan`; rescan/reimport and submit a new job instead.
- On opening Processing, the UI looks up a queued/running/paused job for the
  gallery before submitting. A recovered running job is exposed as paused and
  can be resumed by the user; no active job returns `active_job: null`.
- Queue capacity is configured by `max_running` and `max_queued` at startup.
- The currently wired analyzer is the Rust pixel engine. A raster decode error
  is recorded as a failed photo; it does not fail the whole job.
- Session-scoped plans, calibrated scoring, RAW embedded-preview extraction,
  and long-lived worker supervision are outside this adapter. The runtime
  session layer now protects the mounted analysis routes with Editor bearer
  authorization, but the server remains loopback-only because it has no TLS
  listener or reviewed remote-proxy deployment contract. Origin checks are
  separate from user authentication.
- A limited runtime smoke submitted and completed one `technical` job for two
  synthetic JPEGs (2/2, zero failures); the result appeared under
  `analysis_summary.rust_shadow`. See the
  [runtime evidence](../../../../../../docs/runtime-evidence/2026-11-21-synthetic-persistent-smoke.md).
  A separate manual loopback acceptance confirmed `invalid_plan` responses for
  Start after a queued job's catalog changed and Resume after a paused job was
  recovered and its catalog changed. See the
  [plan revalidation evidence](../../../../../../docs/runtime-evidence/2026-11-21-analysis-plan-revalidation-smoke.md).
  This is not D25 completion: broader pause/recovery cases, larger catalogs,
  RAW, calibrated scoring, and UI behavior remain unaccepted.
