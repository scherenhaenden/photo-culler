//! Framework-independent contract for a headless NAS server launch.
//!
//! This crate does not yet contain `photo-culler-server` as a Cargo package.
//! Keep this module dependency-free so its acceptance harness can run directly
//! with `rustc --test`; the server integrator can adapt these contracts to the
//! eventual Axum runtime and shared API types.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindAddress {
    Loopback,
    Lan(String),
}

impl Default for BindAddress {
    fn default() -> Self {
        Self::Loopback
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThermalSettings {
    pub pause_at_celsius: i16,
    pub resume_below_celsius: i16,
    pub sensor_unavailable_pauses: bool,
}

impl ThermalSettings {
    pub fn validate(&self) -> Result<(), ConfigError> {
        if !(-20..=120).contains(&self.pause_at_celsius) {
            return Err(ConfigError::ThermalPauseOutOfRange);
        }
        if !(-20..=120).contains(&self.resume_below_celsius) {
            return Err(ConfigError::ThermalResumeOutOfRange);
        }
        if self.resume_below_celsius >= self.pause_at_celsius {
            return Err(ConfigError::ThermalHysteresisRequired);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeadlessConfig {
    pub bind: BindAddress,
    /// Required for LAN exposure; treated as an opaque secret by this layer.
    pub admin_token: Option<String>,
    pub catalog_path: PathBuf,
    pub cache_path: PathBuf,
    pub authorized_destinations: Vec<PathBuf>,
    pub thermal: ThermalSettings,
}

impl HeadlessConfig {
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.thermal.validate()?;
        match &self.bind {
            BindAddress::Loopback => Ok(()),
            BindAddress::Lan(host) if host.trim().is_empty() => Err(ConfigError::EmptyLanAddress),
            BindAddress::Lan(_) if self.admin_token.as_deref().is_none_or(str::is_empty) => {
                Err(ConfigError::LanRequiresAdminToken)
            }
            BindAddress::Lan(_) => Ok(()),
        }
    }

    pub fn default_bind(&self) -> &'static str {
        match self.bind {
            BindAddress::Loopback => "127.0.0.1",
            BindAddress::Lan(_) => "explicit-lan-address",
        }
    }

    /// Writable paths are owned by the service; original files remain read-only.
    pub fn permits_write(&self, path: &Path) -> bool {
        path == self.catalog_path
            || path.starts_with(&self.cache_path)
            || self
                .authorized_destinations
                .iter()
                .any(|root| path.starts_with(root))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Unauthenticated,
    Read,
    Edit,
    Admin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    ReadStatus,
    ReadThermal,
    UpdateThermal,
    WriteCatalog,
    WriteDestination,
}

pub fn authorize(
    operation: Operation,
    permission: Permission,
    remote: bool,
) -> Result<(), AuthError> {
    if remote && permission == Permission::Unauthenticated {
        return Err(AuthError::AuthenticationRequired);
    }
    if remote && operation == Operation::UpdateThermal && permission != Permission::Admin {
        return Err(AuthError::RemoteAdminRequired);
    }
    let allowed = match operation {
        Operation::ReadStatus | Operation::ReadThermal => true,
        Operation::UpdateThermal => permission == Permission::Admin,
        Operation::WriteCatalog | Operation::WriteDestination => {
            matches!(permission, Permission::Edit | Permission::Admin)
        }
    };
    if allowed {
        Ok(())
    } else {
        Err(AuthError::PermissionDenied)
    }
}

/// Small startup seam for the runtime adapter. The callback represents binding
/// the socket and opening the catalog, which remain server integration work.
pub fn start<T>(
    config: &HeadlessConfig,
    open: impl FnOnce(&HeadlessConfig) -> T,
) -> Result<T, ConfigError> {
    config.validate()?;
    Ok(open(config))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    ThermalPauseOutOfRange,
    ThermalResumeOutOfRange,
    ThermalHysteresisRequired,
    EmptyLanAddress,
    LanRequiresAdminToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthError {
    RemoteAdminRequired,
    AuthenticationRequired,
    PermissionDenied,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> HeadlessConfig {
        HeadlessConfig {
            bind: BindAddress::default(),
            admin_token: None,
            catalog_path: "/state/catalog.sqlite".into(),
            cache_path: "/cache".into(),
            authorized_destinations: vec!["/exports".into()],
            thermal: ThermalSettings {
                pause_at_celsius: 75,
                resume_below_celsius: 65,
                sensor_unavailable_pauses: true,
            },
        }
    }

    #[test]
    fn startup_defaults_to_loopback_and_opens_catalog_for_persistence() {
        let mut observed = None;
        let result = start(&config(), |cfg| {
            observed = Some((cfg.default_bind(), cfg.catalog_path.clone()));
            // The runtime adapter opens the SQLite catalog here. This double
            // proves startup passes the selected persistent path unchanged.
            "catalog-opened"
        });
        assert_eq!(result, Ok("catalog-opened"));
        assert_eq!(
            observed,
            Some(("127.0.0.1", PathBuf::from("/state/catalog.sqlite")))
        );
    }

    #[test]
    fn originals_are_read_only_and_writes_are_scoped_to_service_storage() {
        let cfg = config();
        assert!(!cfg.permits_write(Path::new("/photos/original.nef")));
        assert!(!cfg.permits_write(Path::new("/cache-escape/evil")));
        assert!(cfg.permits_write(Path::new("/cache/thumbs/a.jpg")));
        assert!(cfg.permits_write(Path::new("/exports/selected.jpg")));
        assert!(cfg.permits_write(Path::new("/state/catalog.sqlite")));
    }

    #[test]
    fn invalid_thermal_settings_stop_startup() {
        let mut cfg = config();
        cfg.thermal.resume_below_celsius = 75;
        assert_eq!(
            start(&cfg, |_| ()),
            Err(ConfigError::ThermalHysteresisRequired)
        );
        cfg.thermal.resume_below_celsius = 64;
        cfg.thermal.pause_at_celsius = 121;
        assert_eq!(cfg.validate(), Err(ConfigError::ThermalPauseOutOfRange));
    }

    #[test]
    fn lan_requires_explicit_address_and_admin_credential() {
        let mut cfg = config();
        cfg.bind = BindAddress::Lan("0.0.0.0".into());
        assert_eq!(cfg.validate(), Err(ConfigError::LanRequiresAdminToken));
        cfg.admin_token = Some("secret-reference".into());
        assert_eq!(cfg.validate(), Ok(()));
    }

    #[test]
    fn remote_configuration_changes_require_admin() {
        assert_eq!(
            authorize(Operation::UpdateThermal, Permission::Edit, true),
            Err(AuthError::RemoteAdminRequired)
        );
        assert_eq!(
            authorize(Operation::UpdateThermal, Permission::Admin, true),
            Ok(())
        );
        assert_eq!(
            authorize(Operation::ReadStatus, Permission::Read, true),
            Ok(())
        );
        assert_eq!(
            authorize(Operation::ReadStatus, Permission::Read, false),
            Ok(())
        );
        assert_eq!(
            authorize(Operation::ReadStatus, Permission::Unauthenticated, true),
            Err(AuthError::AuthenticationRequired)
        );
    }
}
