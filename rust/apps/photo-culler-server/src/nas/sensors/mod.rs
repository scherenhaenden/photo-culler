//! Linux thermal sensor adapter with explicit freshness and failure states.
//!
//! Linux sysfs temperature values are integer millidegrees Celsius. This
//! module keeps sysfs access and the clock behind ports so callers can use
//! synthetic files and clocks in tests.

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SysfsError {
    NotFound,
    Io(String),
}

/// Small port around the filesystem operations needed by sysfs sensors.
pub trait Sysfs {
    fn read_to_string(&self, path: &str) -> Result<String, SysfsError>;
}

/// Monotonic milliseconds since an arbitrary origin.
pub trait Clock {
    fn now_ms(&self) -> u64;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sample {
    /// Value as exposed by Linux sysfs, in thousandths of a degree Celsius.
    pub millidegrees_celsius: i64,
    pub observed_at_ms: u64,
    pub origin: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SampleError {
    Missing,
    Invalid,
    Io(String),
}

/// A sensor port can be implemented by sysfs, a cached provider, or a test.
pub trait Sensor {
    fn sample(&self) -> Result<Option<Sample>, SampleError>;

    fn origin(&self) -> String {
        "sensor".into()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnavailableReason {
    Missing,
    Invalid,
    Expired,
    Io,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Reading {
    /// Null whenever the sensor is unavailable.
    pub temperature_celsius: Option<f64>,
    pub status: Status,
    pub origin: String,
    /// Age is populated when a sample timestamp was available.
    pub age_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Available,
    Unavailable(UnavailableReason),
}

/// Parse and validate a Linux sysfs millidegree value.
pub fn parse_millidegrees(raw: &str) -> Result<i64, ParseError> {
    let value = raw.trim();
    if value.is_empty() {
        return Err(ParseError::Empty);
    }
    let parsed = value.parse::<i64>().map_err(|_| ParseError::Malformed)?;
    // Reject physically implausible readings and guard against garbage values.
    if !(-50_000..=200_000).contains(&parsed) {
        return Err(ParseError::OutOfRange);
    }
    Ok(parsed)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    Empty,
    Malformed,
    OutOfRange,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Empty => "sensor value is empty",
            Self::Malformed => "sensor value is not an integer millidegree value",
            Self::OutOfRange => "sensor value is outside the supported range",
        })
    }
}

/// Read the configured sysfs input and timestamp a successful read.
pub struct LinuxSysfsSensor<F, C> {
    pub filesystem: F,
    pub clock: C,
    pub input_path: String,
}

impl<F: Sysfs, C: Clock> Sensor for LinuxSysfsSensor<F, C> {
    fn sample(&self) -> Result<Option<Sample>, SampleError> {
        let raw = match self.filesystem.read_to_string(&self.input_path) {
            Ok(raw) => raw,
            Err(SysfsError::NotFound) => return Err(SampleError::Missing),
            Err(SysfsError::Io(message)) => return Err(SampleError::Io(message)),
        };
        let millidegrees_celsius = parse_millidegrees(&raw).map_err(|_| SampleError::Invalid)?;
        Ok(Some(Sample {
            millidegrees_celsius,
            observed_at_ms: self.clock.now_ms(),
            origin: self.input_path.clone(),
        }))
    }

    fn origin(&self) -> String {
        self.input_path.clone()
    }
}

/// Build the stable public snapshot. Expired or invalid samples always have a
/// null temperature so consumers cannot mistake old data for live telemetry.
pub fn read<S: Sensor, C: Clock>(sensor: &S, clock: &C, max_age_ms: u64) -> Reading {
    match sensor.sample() {
        Ok(Some(sample)) => {
            let now = clock.now_ms();
            if sample.observed_at_ms > now {
                return unavailable(UnavailableReason::Invalid, sample.origin, None);
            }
            let age_ms = now - sample.observed_at_ms;
            if age_ms > max_age_ms {
                return unavailable(UnavailableReason::Expired, sample.origin, Some(age_ms));
            }
            if !(-50_000..=200_000).contains(&sample.millidegrees_celsius) {
                return unavailable(UnavailableReason::Invalid, sample.origin, Some(age_ms));
            }
            Reading {
                temperature_celsius: Some(sample.millidegrees_celsius as f64 / 1000.0),
                status: Status::Available,
                origin: sample.origin,
                age_ms: Some(age_ms),
            }
        }
        Ok(None) | Err(SampleError::Missing) => {
            unavailable(UnavailableReason::Missing, sensor.origin(), None)
        }
        Err(SampleError::Invalid) => unavailable(UnavailableReason::Invalid, sensor.origin(), None),
        Err(SampleError::Io(_)) => unavailable(UnavailableReason::Io, sensor.origin(), None),
    }
}

fn unavailable(reason: UnavailableReason, origin: String, age_ms: Option<u64>) -> Reading {
    Reading {
        temperature_celsius: None,
        status: Status::Unavailable(reason),
        origin,
        age_ms,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::collections::HashMap;

    #[derive(Clone)]
    struct FakeSysfs(HashMap<String, Result<String, SysfsError>>);
    impl Sysfs for FakeSysfs {
        fn read_to_string(&self, path: &str) -> Result<String, SysfsError> {
            self.0
                .get(path)
                .cloned()
                .unwrap_or(Err(SysfsError::NotFound))
        }
    }

    struct FakeClock(Cell<u64>);
    impl Clock for FakeClock {
        fn now_ms(&self) -> u64 {
            self.0.get()
        }
    }

    struct FixedSensor(Result<Option<Sample>, SampleError>);
    impl Sensor for FixedSensor {
        fn sample(&self) -> Result<Option<Sample>, SampleError> {
            self.0.clone()
        }

        fn origin(&self) -> String {
            self.0
                .as_ref()
                .ok()
                .and_then(|sample| sample.as_ref())
                .map(|sample| sample.origin.clone())
                .unwrap_or_else(|| "fake-sensor".into())
        }
    }

    #[test]
    fn sysfs_valid_read_exposes_celsius_origin_and_age() {
        let mut files = HashMap::new();
        files.insert(
            "/sys/class/thermal/thermal_zone0/temp".into(),
            Ok("64500\n".into()),
        );
        let sensor = LinuxSysfsSensor {
            filesystem: FakeSysfs(files),
            clock: FakeClock(Cell::new(1_000)),
            input_path: "/sys/class/thermal/thermal_zone0/temp".into(),
        };
        let clock = FakeClock(Cell::new(1_025));
        let result = read(&sensor, &clock, 100);
        assert_eq!(result.temperature_celsius, Some(64.5));
        assert_eq!(result.status, Status::Available);
        assert_eq!(result.origin, "/sys/class/thermal/thermal_zone0/temp");
        assert_eq!(result.age_ms, Some(25));
    }

    #[test]
    fn missing_empty_corrupt_and_out_of_range_values_are_unavailable_with_null_temperature() {
        let clock = FakeClock(Cell::new(100));
        let missing = LinuxSysfsSensor {
            filesystem: FakeSysfs(HashMap::new()),
            clock: FakeClock(Cell::new(100)),
            input_path: "/sys/devices/missing".into(),
        };
        assert_eq!(
            read(&missing, &clock, 50).status,
            Status::Unavailable(UnavailableReason::Missing)
        );
        let cases = [
            ("empty", " \n"),
            ("corrupt", "sixty-two"),
            ("range", "300000"),
        ];
        for (name, raw) in cases {
            let path = format!("/sys/{name}");
            let mut files = HashMap::new();
            files.insert(path.clone(), Ok(raw.into()));
            let sensor = LinuxSysfsSensor {
                filesystem: FakeSysfs(files),
                clock: FakeClock(Cell::new(100)),
                input_path: path,
            };
            let result = read(&sensor, &clock, 50);
            assert_eq!(result.temperature_celsius, None, "{name}");
            assert_eq!(
                result.status,
                Status::Unavailable(UnavailableReason::Invalid),
                "{name}"
            );
        }
    }

    #[test]
    fn expired_cached_sample_has_age_and_no_temperature() {
        let clock = FakeClock(Cell::new(5_000));
        let sensor = FixedSensor(Ok(Some(Sample {
            millidegrees_celsius: 71_000,
            observed_at_ms: 4_000,
            origin: "hwmon0/temp1_input".into(),
        })));
        let result = read(&sensor, &clock, 500);
        assert_eq!(result.temperature_celsius, None);
        assert_eq!(
            result.status,
            Status::Unavailable(UnavailableReason::Expired)
        );
        assert_eq!(result.origin, "hwmon0/temp1_input");
        assert_eq!(result.age_ms, Some(1_000));
    }

    #[test]
    fn io_failure_and_future_timestamp_are_distinct_from_a_valid_reading() {
        let clock = FakeClock(Cell::new(500));
        let failed = FixedSensor(Err(SampleError::Io("permission denied".into())));
        assert_eq!(
            read(&failed, &clock, 100).status,
            Status::Unavailable(UnavailableReason::Io)
        );
        let future = FixedSensor(Ok(Some(Sample {
            millidegrees_celsius: 30_000,
            observed_at_ms: 600,
            origin: "synthetic".into(),
        })));
        assert_eq!(
            read(&future, &clock, 100).status,
            Status::Unavailable(UnavailableReason::Invalid)
        );
    }
}
