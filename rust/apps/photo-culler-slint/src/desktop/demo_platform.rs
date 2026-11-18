//! Cross-platform helpers for locating and invoking the sibling demo server.
//! Paths and arguments are passed to `Command` as structured `OsStr` values;
//! no shell string is constructed.
//! Integration: D16 supplies the process lifecycle using `server_command`;
//! the Slint crate manifest/module tree is not present at the sprint base.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutablePlatform {
    MacOs,
    Linux,
    Windows,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlatformError {
    ClientExecutableHasNoParent,
    MissingSibling(PathBuf),
}

pub fn sibling_server_path(
    client_executable: &Path,
    platform: ExecutablePlatform,
) -> Result<PathBuf, PlatformError> {
    let parent = client_executable
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .ok_or(PlatformError::ClientExecutableHasNoParent)?;
    let name = match platform {
        ExecutablePlatform::MacOs | ExecutablePlatform::Linux => "photo-culler-server",
        ExecutablePlatform::Windows => "photo-culler-server.exe",
    };
    let candidate = parent.join(name);
    if !candidate.is_file() {
        return Err(PlatformError::MissingSibling(candidate));
    }
    candidate
        .canonicalize()
        .map_err(|_| PlatformError::MissingSibling(candidate))
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ServerArguments {
    pub bind_address: Option<String>,
    pub data_dir: Option<PathBuf>,
}

impl ServerArguments {
    pub fn as_os_strings(&self) -> Vec<OsString> {
        let mut args = vec![OsString::from("serve")];
        if let Some(address) = &self.bind_address {
            args.push("--bind".into());
            args.push(address.into());
        }
        if let Some(data_dir) = &self.data_dir {
            args.push("--data-dir".into());
            args.push(data_dir.as_os_str().to_owned());
        }
        args
    }
}

pub fn server_command(executable: PathBuf, arguments: &ServerArguments) -> Command {
    let mut command = Command::new(executable);
    command
        .args(arguments.as_os_strings())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    command
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir(PathBuf);
    impl TempDir {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!("Photo Culler platform tests {unique}"));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn resolves_absolute_sibling_paths_with_spaces_and_windows_suffix() {
        let temp = TempDir::new();
        let client = temp.0.join("Photo Culler Desktop.exe");
        fs::write(&client, b"client").unwrap();
        let windows_server = temp.0.join("photo-culler-server.exe");
        fs::write(&windows_server, b"server").unwrap();
        assert_eq!(
            sibling_server_path(&client, ExecutablePlatform::Windows).unwrap(),
            windows_server.canonicalize().unwrap()
        );
        let unix_server = temp.0.join("photo-culler-server");
        fs::write(&unix_server, b"server").unwrap();
        assert_eq!(
            sibling_server_path(&client, ExecutablePlatform::MacOs).unwrap(),
            unix_server.canonicalize().unwrap()
        );
        assert_eq!(
            sibling_server_path(&client, ExecutablePlatform::Linux).unwrap(),
            unix_server.canonicalize().unwrap()
        );
    }

    #[test]
    fn missing_sibling_reports_path_and_argument_paths_remain_structured() {
        let temp = TempDir::new();
        let client = temp.0.join("Photo Culler Desktop");
        fs::write(&client, b"client").unwrap();
        let missing = sibling_server_path(&client, ExecutablePlatform::Windows).unwrap_err();
        assert_eq!(
            missing,
            PlatformError::MissingSibling(temp.0.join("photo-culler-server.exe"))
        );

        let arguments = ServerArguments {
            bind_address: Some("127.0.0.1:0".into()),
            data_dir: Some(PathBuf::from("/Volumes/My Photos/Catalog One")),
        };
        let command = server_command(
            PathBuf::from("/Applications/Photo Culler/photo-culler-server"),
            &arguments,
        );
        let actual: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            actual,
            [
                "serve",
                "--bind",
                "127.0.0.1:0",
                "--data-dir",
                "/Volumes/My Photos/Catalog One"
            ]
        );
    }
}
