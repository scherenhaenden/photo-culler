# Thermal policy adapter (task 54)

This module is independent of the server crate and queue implementation,
which are not present in the sprint base. It consumes a `TemperatureSensor`
port and a monotonic `Clock`, and emits `Pause`/`Resume` actions for the queue
adapter to persist.

The sensor port currently returns `TemperatureReading { celsius_milli,
origin, observed_at_ms }` or `SensorFailure::{Unavailable, Invalid}`. Task 53
can implement this trait directly or map its reading/status DTO to these
types. Missing, invalid, future-dated, and expired readings use the configured
`UnavailablePolicy`. `Pause` fails safe by pausing running work with the
`SensorUnavailable` cause; `HoldLastDecision` preserves the previous thermal
state. A later valid reading at or below the recovery threshold resumes only
jobs whose queue state records `ThermalHigh` or `SensorUnavailable` as their
pause cause.

Jobs already paused manually are never resumed or overwritten by the thermal
policy. Temperature thresholds use milli-Celsius sensor values and hysteresis:
temperatures between the recovery and pause thresholds preserve the latched
state. Persisted queue transitions should include the cause so restart and
manual actions remain distinguishable.

The integration layer still needs to freeze the shared sensor/job DTOs and
register the module. The acceptance tests use in-memory sensor, clock, and job
snapshot doubles.

Run standalone acceptance from the repository root:

```sh
rustc --edition=2024 --test rust/apps/photo-culler-server/src/nas/thermal_policy/mod.rs -o /tmp/photo-culler-task54-tests
/tmp/photo-culler-task54-tests
```
