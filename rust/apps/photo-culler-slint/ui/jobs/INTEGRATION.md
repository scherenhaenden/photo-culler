# Jobs panel integration

This area contains a standalone `JobsPanel<R>` controller, an isolated Slint
`JobsPanel` component, and a simulated REST service in its unit-test source. The
Slint app crate and typed transport now exist, but the reusable jobs panel is
still not registered or bound to a concrete REST adapter.

## REST boundary

`JobsRest` has two operations: load one authoritative snapshot by stable job ID
and submit one of pause, resume, or cancel. The adapter should map snapshots
from the task 15 job-progress endpoint and commands from task 14. Expected
snapshot fields are job ID, import/rescan/analysis kind, state, phase, completed
and optional total counters, and a monotonic snapshot version. The UI keeps
that ID and its last confirmed snapshot if transport fails; reconnect calls
`connection_restored` and immediately requests the same job's latest snapshot.

The shared client now exposes D25 analysis submit/status/start/pause/resume/
cancel routes and validated aggregate progress. Its DTO does not include the
generic jobs panel's `phase` or monotonic `version`, so a mapper is required
before reusing `JobsPanel<R>`. A small `analysis_flow` is connected on desktop
and browser. Processing submits one technical analysis job per gallery or
refreshes the existing job; queued and running jobs poll every two seconds.
Polling stops after a terminal state or navigation to a different gallery. A
transient status or control error now keeps the tracked ID and re-queries that
same job every two seconds. After a control timeout it refreshes status before
exposing a confirmed state; the pause/resume/cancel command is not resent.
Selecting another gallery immediately clears the old
status and controls while retaining its tracked job; selecting Processing in
that gallery restores its current server status. The status row exposes pause
while running, resume while paused, and cancel while queued, running, or paused. Import jobs are shown
separately in the import dialog.

The controller suppresses another command while an action awaits a confirming
server state. It also disables actions that are invalid for the confirmed job
state. A rejected action keeps the last confirmed snapshot and schedules an
immediate refresh. The controller does not create a new job while reconnecting.

`JobsRest` is a synchronous port for deterministic controller logic and tests.
Do not call `poll` or `send_action` on Slint's event/UI thread with a network
adapter: execute REST calls on a worker/executor, then marshal the result and
controller state update back onto the Slint event loop before changing component
properties. The eventual async adapter should preserve the same serialized
action guard and job-ID-based resynchronization behavior.

The generic jobs controller's suggested cadence is 500 ms for queued/running/
cancelling jobs, 1.5 s for paused jobs, and 5 s for terminal or idle state.
The current D25 app flow uses a two-second cadence while queued/running and
retries the same snapshot after a transient error. On page reopen it asks the
server for one active job in the selected gallery before submitting; the
server returns the most recently updated queued/running/paused snapshot. It
does not yet implement exponential offline backoff or present a chooser if
multiple active jobs exist for the same gallery. The reusable jobs panel's
adaptive cadence and controller mapping remain unintegrated. The view can
display progress as indeterminate when total is unknown; the numeric progress
property is finite and clamped to 0–1.

## Tests

The module tests use a fake REST service and can be run from the repository
root without the pending Slint application crate:

```sh
rustc --edition=2024 -D warnings --test rust/apps/photo-culler-slint/ui/jobs/mod.rs \
  -o /private/tmp/photo-culler-task49-jobs-tests
/private/tmp/photo-culler-task49-jobs-tests
```

They cover queued/running/terminal progress, adaptive polling, disconnect and
resync with the same job ID, pause/resume/cancel, duplicate action suppression,
and REST action failure. They have not been run in this task. The reusable jobs
component's registration, mapping, adaptive polling, and reconnect behavior
still need integration and acceptance. The D25 flow change is source-level
progress only; it has not received a build or visual/runtime acceptance against
a running analysis service.
