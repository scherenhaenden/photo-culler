# Thermal policy adapter (task 54)

This module is independent of the server crate and queue implementation,
which are not present in the sprint base. It consumes a `TemperatureSensor`
port and a monotonic `Clock`, and emits `Pause`/`Resume` actions for the queue
adapter to persist.

Task 53's sensor port is `read(sensor, clock, max_age_ms) -> Reading`, with
`temperature_celsius: Option<f64>`, status, origin, and age. The server adapter
should map an `Available` reading with a finite temperature to
`TemperatureReading`: round Celsius times 1000 into `celsius_milli`, and set
`observed_at_ms` to the monotonic policy clock minus `age_ms` (saturating at
zero). Map `Invalid` to `SensorFailure::Invalid`; map `Unavailable(Missing)`,
`Unavailable(Expired)`, and `Unavailable(Io)` to
`SensorFailure::Unavailable`. An `Available` reading without a temperature
maps to `Invalid`. The policy then applies `UnavailablePolicy`: `Pause` fails
safe by pausing running work with the `SensorUnavailable` cause;
`HoldLastDecision` preserves the previous thermal state. A later valid reading
at or below the recovery threshold resumes only jobs whose queue state records
`ThermalHigh` or `SensorUnavailable` as their pause cause.

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
