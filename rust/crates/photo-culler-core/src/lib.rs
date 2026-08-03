//! Shared, UI-independent contracts for incremental Rust adoption.

use std::{fmt, path::PathBuf, str::FromStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageBackend {
    Sqlite,
    PostgreSql,
}

impl StorageBackend {
    pub const fn readiness(self) -> u8 {
        match self {
            Self::Sqlite => 70,
            Self::PostgreSql => 5,
        }
    }
}

impl fmt::Display for StorageBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Sqlite => "sqlite",
            Self::PostgreSql => "postgresql",
        })
    }
}

impl FromStr for StorageBackend {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_ascii_lowercase().as_str() {
            "sqlite" => Ok(Self::Sqlite),
            "postgres" | "postgresql" => Ok(Self::PostgreSql),
            other => Err(format!("unsupported storage backend: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Frontend {
    TauriWebGl,
    EguiWgpu,
}

impl Frontend {
    pub const fn readiness(self) -> u8 {
        match self {
            Self::TauriWebGl => 7,
            Self::EguiWgpu => 60,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalysisRequest {
    pub photo_id: String,
    pub source: PathBuf,
    pub profile: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetricResult {
    pub analyzer: String,
    pub implementation_version: String,
    pub value: f64,
    pub confidence: f64,
    pub elapsed_micros: u64,
}

pub trait AnalysisEngine {
    fn name(&self) -> &'static str;
    fn analyze(&self, request: &AnalysisRequest) -> Result<Vec<MetricResult>, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_storage_backends() {
        assert_eq!("sqlite".parse(), Ok(StorageBackend::Sqlite));
        assert_eq!("postgres".parse(), Ok(StorageBackend::PostgreSql));
        assert!("mysql".parse::<StorageBackend>().is_err());
    }

    #[test]
    fn frontend_readiness_matches_the_verified_delivery_scope() {
        assert!(Frontend::TauriWebGl.readiness() < 10);
        assert_eq!(Frontend::EguiWgpu.readiness(), 60);
    }
}
