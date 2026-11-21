# D25 analysis REST client

`demo::DemoClient` exposes the persistent analysis-job contract through the
same-origin `Transport`:

- `submit_analysis_job(gallery_id, job_id, profile_id)` submits a job and
  verifies that the server returned the requested gallery, job and profile.
- `active_analysis_job(gallery_id)` discovers one queued, running, or paused
  job for reconnecting after application restart; it returns `None` when no
  recoverable job exists. The server selects the most recently updated active
  job if a gallery has more than one.
- `analysis_job(catalog_id, job_id)` retrieves aggregate status.
- `start_analysis_job`, `pause_analysis_job`, `resume_analysis_job` and
  `cancel_analysis_job` return the updated job snapshot.

The client accepts only 1–128 character ASCII IDs containing letters, digits,
`-`, `_` or `.`, and rejects `..`. The submit JSON is capped at 4 KiB. All
responses use the existing JSON size limit, reject redirects, require a JSON
MIME type, and surface server failures through `ClientError::Http` with the
typed error body when available. Response validation checks contract version,
requested IDs, known states and consistent completed/failed/total/percentage
values. The API returns aggregate progress only; source paths and raw analyzer
errors are not part of the client model.

Submission does not start a job implicitly. The application discovers an
existing active job before submitting, starts a recovered queued job, and can
poll `analysis_job` at an appropriate cadence. Runtime authentication and
server route mounting remain responsibilities of the application/server
integration.
