# Task 53 sensor integration notes

This module is a transport-independent Linux sensor adapter. It does not
register itself in the server crate because that crate and its root module are
not present on the sprint base.

`LinuxSysfsSensor` reads a configured sysfs input such as
`/sys/class/thermal/thermal_zone0/temp` or a hwmon `temp*_input` path. The
injected `Sysfs` port supplies file contents and `Clock` timestamps each
successful read. Linux millidegree values are checked against the supported
range before conversion to Celsius.

The stable snapshot for policy and diagnostics consumers is `Reading`:

- `temperature_celsius` is present only for a fresh valid sample; it remains
  `None` for every unavailable outcome.
- `status` distinguishes `Missing`, `Invalid`, `Expired`, and `Io` failures.
- `origin` identifies the configured sysfs path or injected sensor source.
- `age_ms` is present when a sample timestamp exists.

Task 54 can consume `read(sensor, clock, max_age_ms)` and should treat every
`Unavailable` status according to its configured sensor-failure policy. The
module deliberately leaves serialized field names, Serde types, source
discovery, and the server's `pub mod sensors;` registration to integration.

Standalone acceptance from the repository root:

```sh
rustc --edition=2024 --test rust/apps/photo-culler-server/src/nas/sensors/mod.rs -o /tmp/photo-culler-task53-tests
/tmp/photo-culler-task53-tests
```
