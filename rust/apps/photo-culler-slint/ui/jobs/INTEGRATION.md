# Jobs panel integration

At base `dfb7434`, the shared Slint UI components and `photo-culler-client`
crate are present. The `photo-culler-slint` application root, its manifest and
component aggregator are still pending. This area contains a standalone
`JobsPanel<R>` controller, an isolated Slint `JobsPanel` component, and a
simulated REST service in unit tests. The application integrator still needs
to register the component, bind its properties/callbacks to this controller,
and supply the concrete job client.

## REST boundary

`JobsRest` has two operations: load one authoritative snapshot by stable job ID
and submit one of pause, resume, or cancel. The adapter should map snapshots
from the task 15 job-progress endpoint and commands from task 14. Expected
snapshot fields are job ID, import/rescan/analysis kind, state, phase, completed
and optional total counters, and a monotonic snapshot version. The UI keeps
that ID and its last confirmed snapshot if transport fails; reconnect calls
`connection_restored` and immediately requests the same job's latest snapshot.

The shared `RestClient<T>` currently exposes typed gallery GET requests only;
its `Transport` port has no write operation, and it has no job snapshot/command
DTOs. Server tasks 14 and 15 (job commands and progress endpoints) are also
pending. The integration change must extend or adapt the shared client for job
GET/action calls before binding it to `JobsRest`; this task intentionally keeps
the fakeable port local to the jobs area.

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

Polling cadence is 500 ms for queued/running/cancelling jobs, 1.5 s for paused
jobs, and 5 s for terminal or idle state. Transport failures back off from 1 s
to a 15 s cap. The view can display progress as indeterminate when total is
unknown; the numeric progress property is finite and clamped to 0–1.

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
and REST action failure. The components from task 41 are available for a later
restyle, while compiling this `.slint` file and exercising native/WASM bindings
remain pending until the Slint app root, manifest, and component aggregator are
added by the integrator.
