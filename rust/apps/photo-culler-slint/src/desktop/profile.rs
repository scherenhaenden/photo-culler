//! Persistent, non-secret desktop preferences.
//!
//! The local service owns the catalog database. This file stores only the
//! selected source root and gallery identity; session credentials and ports
//! must never be written here.

use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    ffi::OsString,
    fmt,
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub const PROFILE_SCHEMA_VERSION: u32 = 1;
const PROFILE_FILE_NAME: &str = "desktop.json";
static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DesktopProfile {
    schema_version: u32,
    /// UTF-8 absolute path selected by the user. Kept as text so conversion
    /// from a platform path is explicit and can reject data loss.
    pub authorized_root: Option<String>,
    pub gallery_id: Option<String>,
}

impl DesktopProfile {
    pub fn empty() -> Self {
        Self {
            schema_version: PROFILE_SCHEMA_VERSION,
            authorized_root: None,
            gallery_id: None,
        }
    }

    pub fn for_selection(root: &Path, gallery_id: Option<&str>) -> Result<Self, ProfileError> {
        if !root.is_absolute() {
            return Err(ProfileError::InvalidProfile(
                "authorized root must be an absolute path".into(),
            ));
        }
        let root = root.to_str().ok_or(ProfileError::NonUtf8Path)?;
        let gallery_id = gallery_id.map(str::trim);
        if gallery_id.is_some_and(str::is_empty) {
            return Err(ProfileError::InvalidProfile(
                "gallery ID must not be empty".into(),
            ));
        }
        Ok(Self {
            schema_version: PROFILE_SCHEMA_VERSION,
            authorized_root: Some(root.to_owned()),
            gallery_id: gallery_id.map(str::to_owned),
        })
    }

    pub fn schema_version(&self) -> u32 {
        self.schema_version
    }

    pub fn authorized_root_path(&self) -> Option<PathBuf> {
        self.authorized_root.as_deref().map(PathBuf::from)
    }

    fn validate(&self) -> Result<(), ProfileError> {
        if self.schema_version != PROFILE_SCHEMA_VERSION {
            return Err(ProfileError::UnsupportedVersion(self.schema_version));
        }
        if let Some(root) = &self.authorized_root {
            if root.is_empty() || !Path::new(root).is_absolute() {
                return Err(ProfileError::InvalidProfile(
                    "authorized root must be a non-empty absolute path".into(),
                ));
            }
        }
        if self
            .gallery_id
            .as_deref()
            .is_some_and(|id| id.trim().is_empty())
        {
            return Err(ProfileError::InvalidProfile(
                "gallery ID must not be empty".into(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
pub enum ProfileError {
    Io(io::Error),
    Json(serde_json::Error),
    NonUtf8Path,
    UnsupportedVersion(u32),
    InvalidProfile(String),
}

impl fmt::Display for ProfileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "desktop profile I/O failed: {error}"),
            Self::Json(error) => write!(f, "desktop profile is invalid JSON: {error}"),
            Self::NonUtf8Path => f.write_str("selected path cannot be represented as UTF-8"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported desktop profile schema version {version}")
            }
            Self::InvalidProfile(message) => write!(f, "invalid desktop profile: {message}"),
        }
    }
}

impl Error for ProfileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for ProfileError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ProfileError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Returns the platform's stable profile directory without creating it.
/// Linux follows XDG_DATA_HOME when it is absolute; macOS uses Application
/// Support. This path is separate from the service's `catalog.sqlite3` data.
pub fn default_profile_dir() -> Result<PathBuf, ProfileError> {
    #[cfg(target_os = "linux")]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        let xdg = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
        profile_dir_for(DesktopPlatform::Linux, home.as_deref(), xdg.as_deref())
    }
    #[cfg(target_os = "macos")]
    {
        let home = std::env::var_os("HOME").map(PathBuf::from);
        profile_dir_for(DesktopPlatform::MacOs, home.as_deref(), None)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        Err(ProfileError::InvalidProfile(
            "desktop profiles are supported only on Linux and macOS".into(),
        ))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DesktopPlatform {
    Linux,
    MacOs,
}

/// Pure path resolver kept public to make platform behavior testable on CI.
pub fn profile_dir_for(
    platform: DesktopPlatform,
    home: Option<&Path>,
    xdg_data_home: Option<&Path>,
) -> Result<PathBuf, ProfileError> {
    match platform {
        DesktopPlatform::Linux => {
            if let Some(xdg) = xdg_data_home.filter(|path| path.is_absolute()) {
                return Ok(xdg.join("photo-culler").join("desktop"));
            }
            let home = home.ok_or_else(|| {
                ProfileError::InvalidProfile("HOME is not set for Linux desktop".into())
            })?;
            Ok(home.join(".local/share/photo-culler/desktop"))
        }
        DesktopPlatform::MacOs => {
            let home = home.ok_or_else(|| {
                ProfileError::InvalidProfile("HOME is not set for macOS desktop".into())
            })?;
            Ok(home.join("Library/Application Support/Photo Culler/Desktop"))
        }
    }
}

pub fn profile_file_path(profile_dir: &Path) -> PathBuf {
    profile_dir.join(PROFILE_FILE_NAME)
}

/// Loads a profile. `Ok(None)` means first launch; malformed or unknown
/// versions remain errors so callers can show recovery UI without overwriting.
pub fn load(path: &Path) -> Result<Option<DesktopProfile>, ProfileError> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let profile: DesktopProfile = serde_json::from_slice(&bytes)?;
    profile.validate()?;
    Ok(Some(profile))
}

/// Writes JSON through a unique temporary file in the same directory, fsyncs
/// it, and atomically renames it over the old profile.
pub fn save(path: &Path, profile: &DesktopProfile) -> Result<(), ProfileError> {
    profile.validate()?;
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| {
            ProfileError::InvalidProfile("profile path has no parent directory".into())
        })?;
    fs::create_dir_all(parent)?;
    let bytes = serde_json::to_vec_pretty(profile)?;
    let temp_path = temporary_path(path);
    let write_result = (|| -> Result<(), ProfileError> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp_path, path)?;
        // Best effort durability for the directory entry after rename.
        if let Ok(directory) = OpenOptions::new().read(true).open(parent) {
            let _ = directory.sync_all();
        }
        Ok(())
    })();
    if write_result.is_err() {
        let _ = fs::remove_file(&temp_path);
    }
    write_result
}

fn temporary_path(path: &Path) -> PathBuf {
    let tick = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let mut name = OsString::from(".");
    name.push(path.file_name().unwrap_or_default());
    name.push(format!(".{}.{}.{}.tmp", std::process::id(), tick, sequence));
    path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, path::PathBuf};

    fn temp_dir(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "photo-culler-profile-{label}-{}-{nonce}",
            std::process::id()
        ))
    }

    #[test]
    fn linux_and_macos_use_expected_profile_locations() {
        let home = Path::new("/home/test");
        assert_eq!(
            profile_dir_for(DesktopPlatform::Linux, Some(home), Some(Path::new("/data"))).unwrap(),
            Path::new("/data/photo-culler/desktop")
        );
        assert_eq!(
            profile_dir_for(
                DesktopPlatform::Linux,
                Some(home),
                Some(Path::new("relative"))
            )
            .unwrap(),
            Path::new("/home/test/.local/share/photo-culler/desktop")
        );
        assert_eq!(
            profile_dir_for(DesktopPlatform::MacOs, Some(home), None).unwrap(),
            Path::new("/home/test/Library/Application Support/Photo Culler/Desktop")
        );
    }

    #[test]
    fn missing_profile_is_first_launch_and_selection_round_trips() {
        let dir = temp_dir("roundtrip");
        let file = profile_file_path(&dir);
        assert_eq!(load(&file).unwrap(), None);
        let profile =
            DesktopProfile::for_selection(Path::new("/photos/holiday"), Some("gallery-1")).unwrap();
        save(&file, &profile).unwrap();
        let loaded = load(&file).unwrap().unwrap();
        assert_eq!(loaded.schema_version(), PROFILE_SCHEMA_VERSION);
        assert_eq!(
            loaded.authorized_root_path(),
            Some(PathBuf::from("/photos/holiday"))
        );
        assert_eq!(loaded.gallery_id.as_deref(), Some("gallery-1"));
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn unsupported_version_is_reported_without_rewriting_file() {
        let dir = temp_dir("version");
        fs::create_dir_all(&dir).unwrap();
        let file = profile_file_path(&dir);
        let original = br#"{"schema_version":99,"authorized_root":null,"gallery_id":null}"#;
        fs::write(&file, original).unwrap();
        assert!(matches!(
            load(&file),
            Err(ProfileError::UnsupportedVersion(99))
        ));
        assert_eq!(fs::read(&file).unwrap(), original);
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_or_non_utf8_selected_roots_are_rejected() {
        assert!(matches!(
            DesktopProfile::for_selection(Path::new("relative/photos"), None),
            Err(ProfileError::InvalidProfile(_))
        ));
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            let non_utf8 = PathBuf::from(std::ffi::OsStr::from_bytes(b"/photos/\xff"));
            assert!(matches!(
                DesktopProfile::for_selection(&non_utf8, None),
                Err(ProfileError::NonUtf8Path)
            ));
        }
    }
}
