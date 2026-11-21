//! Local filesystem adapter for reviewed, derived-only exports.
//!
//! This adapter writes only below one canonical destination root and rejects
//! roots that overlap configured original/source roots. Each output is created
//! exclusively; it never replaces an existing file.

use super::DerivedWriter;
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct LocalDerivedWriter {
    root: PathBuf,
}

impl LocalDerivedWriter {
    /// Open an existing, explicitly approved destination root. Source roots are
    /// canonicalized too; equal, nested, or containing roots are refused so an
    /// export tree cannot overlap originals.
    pub fn new(
        destination_root: impl AsRef<Path>,
        source_roots: impl IntoIterator<Item = impl AsRef<Path>>,
    ) -> Result<Self, String> {
        let root = fs::canonicalize(destination_root.as_ref())
            .map_err(|error| format!("cannot resolve export root: {error}"))?;
        let metadata =
            fs::metadata(&root).map_err(|error| format!("cannot inspect export root: {error}"))?;
        if !metadata.is_dir() {
            return Err("export root must be an existing directory".into());
        }

        for source_root in source_roots {
            let source = fs::canonicalize(source_root.as_ref())
                .map_err(|error| format!("cannot resolve configured source root: {error}"))?;
            if root.starts_with(&source) || source.starts_with(&root) {
                return Err("export root must not overlap an original source root".into());
            }
        }

        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn destination_path(&self, destination: &str) -> Result<PathBuf, String> {
        let path = Path::new(destination);
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| "export destination is outside the approved root".to_owned())?;
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err("export destination is not a safe relative file path".into());
        }
        Ok(relative.to_path_buf())
    }

    fn checked_target(&self, destination: &str) -> Result<(PathBuf, PathBuf), String> {
        let relative = self.destination_path(destination)?;
        let target = self.root.join(relative);
        let parent = target
            .parent()
            .ok_or_else(|| "export destination has no parent directory".to_owned())?
            .to_path_buf();
        self.check_existing_ancestors(&parent)?;
        Ok((target, parent))
    }

    fn check_existing_ancestors(&self, path: &Path) -> Result<(), String> {
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| "export destination is outside the approved root".to_owned())?;
        let mut current = self.root.clone();
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err("export destination is not a safe relative path".into());
            };
            current.push(name);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    return Err("export destination traverses a symbolic link".into());
                }
                Ok(metadata) if !metadata.is_dir() => {
                    return Err("export destination parent is not a directory".into());
                }
                Ok(_) => {
                    let canonical = fs::canonicalize(&current)
                        .map_err(|error| format!("cannot resolve export directory: {error}"))?;
                    if !canonical.starts_with(&self.root) {
                        return Err("export destination resolves outside the approved root".into());
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => return Err(format!("cannot inspect export directory: {error}")),
            }
        }
        Ok(())
    }
}

impl DerivedWriter for LocalDerivedWriter {
    fn exists(&self, destination: &str) -> Result<bool, String> {
        let (target, _) = self.checked_target(destination)?;
        match fs::symlink_metadata(&target) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err("export destination is a symbolic link".into());
                }
                let canonical = fs::canonicalize(&target)
                    .map_err(|error| format!("cannot resolve export destination: {error}"))?;
                if !canonical.starts_with(&self.root) {
                    return Err("export destination resolves outside the approved root".into());
                }
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(format!("cannot inspect export destination: {error}")),
        }
    }

    fn create_new(&mut self, destination: &str, bytes: &[u8]) -> Result<(), String> {
        let (target, parent) = self.checked_target(destination)?;
        create_directories_beneath(&self.root, &parent)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|error| match error.kind() {
                std::io::ErrorKind::AlreadyExists => "export destination already exists".into(),
                _ => format!("cannot create derived export: {error}"),
            })?;
        file.write_all(bytes)
            .map_err(|error| format!("cannot write derived export: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("cannot flush derived export: {error}"))
    }
}

fn create_directories_beneath(root: &Path, parent: &Path) -> Result<(), String> {
    let relative = parent
        .strip_prefix(root)
        .map_err(|_| "export destination is outside the approved root".to_owned())?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return Err("export destination is not a safe relative path".into());
        };
        current.push(name);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err("export destination traverses a symbolic link".into());
            }
            Ok(metadata) if !metadata.is_dir() => {
                return Err("export destination parent is not a directory".into());
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                match fs::create_dir(&current) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                    Err(error) => return Err(format!("cannot create export directory: {error}")),
                }
            }
            Err(error) => return Err(format!("cannot inspect export directory: {error}")),
        }
        let canonical = fs::canonicalize(&current)
            .map_err(|error| format!("cannot resolve export directory: {error}"))?;
        if !canonical.starts_with(root) {
            return Err("export destination resolves outside the approved root".into());
        }
    }
    Ok(())
}
