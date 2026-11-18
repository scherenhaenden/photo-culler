//! Thermal job policy. This module emits decisions; job persistence and queue
//! transitions remain the responsibility of the server adapter.

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// At or above this temperature, running jobs are paused.
    pub pause_at_celsius: f32,
    /// At or below this temperature, jobs paused by this policy may resume.
    pub resume_at_celsius: f32,
    /// Sensor readings older than this monotonic age are treated as unavailable.
    pub max_age_ms: u64,
    pub unavailable: UnavailablePolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnavailablePolicy {
    /// Fail safe: pause running jobs until a fresh valid reading returns.
    Pause,
    /// Keep the last thermal decision while the sensor is unavailable.
    HoldLastDecision,
}

impl Config {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if !self.pause_at_celsius.is_finite() || !self.resume_at_celsius.is_finite() {
            return Err(ConfigError::NonFiniteThreshold);
        }
        if self.resume_at_celsius >= self.pause_at_celsius {
            return Err(ConfigError::InvalidHysteresis);
        }
        if self.max_age_ms == 0 {
            return Err(ConfigError::ZeroMaxAge);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfigError {
    NonFiniteThreshold,
    InvalidHysteresis,
    ZeroMaxAge,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TemperatureReading {
    pub celsius_milli: i32,
    pub origin: String,
    pub observed_at_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SensorFailure {
    Unavailable,
    Invalid,
}

pub trait TemperatureSensor {
    fn read(&self) -> Result<TemperatureReading, SensorFailure>;
}

pub trait Clock {
    fn now_ms(&self) -> u64;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PauseCause {
    Manual,
    ThermalHigh,
    SensorUnavailable,
}

impl PauseCause {
    fn is_policy_owned(self) -> bool {
        matches!(self, Self::ThermalHigh | Self::SensorUnavailable)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobState {
    Running,
    Paused(PauseCause),
    Terminal,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub id: String,
    pub state: JobState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Pause { job_id: String, cause: PauseCause },
    Resume { job_id: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ThermalState {
    Unknown,
    Normal,
    High,
    SensorUnavailable,
}

pub struct ThermalPolicy {
    config: Config,
    state: ThermalState,
}

impl ThermalPolicy {
    pub fn new(config: Config) -> Result<Self, ConfigError> {
        config.validate()?;
        Ok(Self {
            config,
            state: ThermalState::Unknown,
        })
    }

    /// Reconcile one sensor observation against a queue snapshot. Manual
    /// pauses are never resumed by this policy. Call once per policy tick.
    pub fn reconcile<S: TemperatureSensor, C: Clock>(
        &mut self,
        sensor: &S,
        clock: &C,
        jobs: &[Job],
    ) -> Vec<Action> {
        self.state = self.next_state(sensor.read(), clock.now_ms());
        let desired_cause = match self.state {
            ThermalState::Unknown => None,
            ThermalState::Normal => None,
            ThermalState::High => Some(PauseCause::ThermalHigh),
            ThermalState::SensorUnavailable => Some(PauseCause::SensorUnavailable),
        };

        let mut actions = Vec::new();
        for job in jobs {
            match (&job.state, desired_cause) {
                (JobState::Running, Some(cause)) => actions.push(Action::Pause {
                    job_id: job.id.clone(),
                    cause,
                }),
                (JobState::Paused(cause), None)
                    if self.state == ThermalState::Normal && cause.is_policy_owned() =>
                {
                    actions.push(Action::Resume {
                        job_id: job.id.clone(),
                    });
                }
                _ => {}
            }
        }
        actions
    }

    fn next_state(
        &self,
        sample: Result<TemperatureReading, SensorFailure>,
        now_ms: u64,
    ) -> ThermalState {
        let reading = match sample {
            Ok(reading) if !reading.origin.is_empty() => reading,
            Ok(_) => return self.unavailable_state(SensorFailure::Invalid),
            Err(failure) => return self.unavailable_state(failure),
        };
        if reading.observed_at_ms > now_ms
            || now_ms - reading.observed_at_ms > self.config.max_age_ms
        {
            return self.unavailable_state(SensorFailure::Unavailable);
        }
        let temp = reading.celsius_milli as f32 / 1000.0;
        if temp >= self.config.pause_at_celsius {
            ThermalState::High
        } else if temp <= self.config.resume_at_celsius {
            ThermalState::Normal
        } else {
            self.state
        }
    }

    fn unavailable_state(&self, _failure: SensorFailure) -> ThermalState {
        match self.config.unavailable {
            UnavailablePolicy::Pause => ThermalState::SensorUnavailable,
            UnavailablePolicy::HoldLastDecision => self.state,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    struct FakeSensor(Cell<Result<i32, SensorFailure>>);
    impl TemperatureSensor for FakeSensor {
        fn read(&self) -> Result<TemperatureReading, SensorFailure> {
            self.0.get().map(|celsius_milli| TemperatureReading {
                celsius_milli,
                origin: "fake/sysfs/temp1_input".into(),
                observed_at_ms: 1_000,
            })
        }
    }
    struct FakeClock(u64);
    impl Clock for FakeClock {
        fn now_ms(&self) -> u64 {
            self.0
        }
    }
    fn policy(unavailable: UnavailablePolicy) -> ThermalPolicy {
        ThermalPolicy::new(Config {
            pause_at_celsius: 80.0,
            resume_at_celsius: 70.0,
            max_age_ms: 5_000,
            unavailable,
        })
        .unwrap()
    }
    fn job(id: &str, state: JobState) -> Job {
        Job {
            id: id.into(),
            state,
        }
    }

    #[test]
    fn pauses_at_high_threshold_and_hysteresis_ignores_oscillation_until_recovery() {
        let sensor = FakeSensor(Cell::new(Ok(80_000)));
        let clock = FakeClock(1_000);
        let mut policy = policy(UnavailablePolicy::Pause);
        let running = [job("j1", JobState::Running)];
        assert_eq!(
            policy.reconcile(&sensor, &clock, &running),
            vec![Action::Pause {
                job_id: "j1".into(),
                cause: PauseCause::ThermalHigh
            }]
        );
        let paused = [job("j1", JobState::Paused(PauseCause::ThermalHigh))];
        for temp in [79_000, 75_000, 70_001] {
            sensor.0.set(Ok(temp));
            assert!(policy.reconcile(&sensor, &clock, &paused).is_empty());
        }
        sensor.0.set(Ok(70_000));
        assert_eq!(
            policy.reconcile(&sensor, &clock, &paused),
            vec![Action::Resume {
                job_id: "j1".into()
            }]
        );
    }

    #[test]
    fn recovered_temperature_never_resumes_a_manual_pause() {
        let sensor = FakeSensor(Cell::new(Ok(90_000)));
        let clock = FakeClock(1_000);
        let mut policy = policy(UnavailablePolicy::Pause);
        let manual = [job("manual", JobState::Paused(PauseCause::Manual))];
        assert!(policy.reconcile(&sensor, &clock, &manual).is_empty());
        sensor.0.set(Ok(65_000));
        assert!(policy.reconcile(&sensor, &clock, &manual).is_empty());
    }

    #[test]
    fn unavailable_policy_pauses_running_jobs_and_recovers_only_policy_pause() {
        let sensor = FakeSensor(Cell::new(Err(SensorFailure::Invalid)));
        let clock = FakeClock(1_000);
        let mut policy = policy(UnavailablePolicy::Pause);
        let running = [job("j1", JobState::Running)];
        assert_eq!(
            policy.reconcile(&sensor, &clock, &running),
            vec![Action::Pause {
                job_id: "j1".into(),
                cause: PauseCause::SensorUnavailable
            }]
        );
        sensor.0.set(Ok(65_000));
        let paused = [job("j1", JobState::Paused(PauseCause::SensorUnavailable))];
        assert_eq!(
            policy.reconcile(&sensor, &clock, &paused),
            vec![Action::Resume {
                job_id: "j1".into()
            }]
        );
    }

    #[test]
    fn stale_sensor_reading_uses_configured_hold_policy() {
        let sensor = FakeSensor(Cell::new(Ok(90_000)));
        let stale_clock = FakeClock(10_000);
        let mut policy = policy(UnavailablePolicy::HoldLastDecision);
        let running = [job("j1", JobState::Running)];
        assert!(policy.reconcile(&sensor, &stale_clock, &running).is_empty());
        assert!(matches!(
            ThermalPolicy::new(Config {
                pause_at_celsius: 70.0,
                resume_at_celsius: 70.0,
                max_age_ms: 2,
                unavailable: UnavailablePolicy::Pause
            }),
            Err(ConfigError::InvalidHysteresis)
        ));
    }

    #[test]
    fn startup_inside_hysteresis_band_does_not_resume_recovered_jobs() {
        let sensor = FakeSensor(Cell::new(Ok(75_000)));
        let clock = FakeClock(1_000);
        let mut policy = policy(UnavailablePolicy::Pause);
        let thermal_paused = [job("j1", JobState::Paused(PauseCause::ThermalHigh))];
        assert!(
            policy
                .reconcile(&sensor, &clock, &thermal_paused)
                .is_empty()
        );
    }
}
