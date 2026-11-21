//! Source availability and relocation. Persistence and platform volume identity are
//! supplied by the integrating server; no paths or decisions are deleted here.
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Online,
    Offline,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error<E> {
    InvalidRelativePath,
    IdentityMismatch,
    Probe(E),
}

/// Implementations must identify mounted volumes independently of their mount path.
/// `volume_id` returning None means no volume is mounted at that path. Permission
/// and I/O failures must be returned as errors, never translated into absence.
pub trait FileSystem {
    type Error;
    fn volume_id(&self, mount: &Path) -> Result<Option<String>, Self::Error>;
    fn source_id(&self, root: &Path) -> Result<Option<String>, Self::Error>;
    fn file_exists(&self, file: &Path) -> Result<bool, Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub volume_id: String,
    pub source_id: String,
    pub mount: PathBuf,
    pub relative_root: PathBuf,
}

fn valid_relative(path: &Path) -> bool {
    !path.as_os_str().is_empty() && path.components().all(|c| matches!(c, Component::Normal(_)))
}

impl Source {
    /// Require an explicitly identified volume and source and a confined relative root.
    pub fn new<E>(
        volume_id: String,
        source_id: String,
        mount: PathBuf,
        relative_root: PathBuf,
    ) -> Result<Self, Error<E>> {
        if !mount.is_absolute()
            || !valid_relative(&relative_root)
            || volume_id.is_empty()
            || source_id.is_empty()
        {
            return Err(Error::InvalidRelativePath);
        }
        Ok(Self {
            volume_id,
            source_id,
            mount,
            relative_root,
        })
    }

    /// Verify both identities before committing a new mount path.
    pub fn relocate<F: FileSystem>(
        &mut self,
        fs: &F,
        mount: PathBuf,
    ) -> Result<(), Error<F::Error>> {
        if !mount.is_absolute() {
            return Err(Error::InvalidRelativePath);
        }
        if fs.volume_id(&mount).map_err(Error::Probe)?.as_deref() != Some(&self.volume_id)
            || fs
                .source_id(&mount.join(&self.relative_root))
                .map_err(Error::Probe)?
                .as_deref()
                != Some(&self.source_id)
        {
            return Err(Error::IdentityMismatch);
        }
        self.mount = mount;
        Ok(())
    }

    pub fn file_status<F: FileSystem>(
        &self,
        fs: &F,
        relative_file: &Path,
    ) -> Result<Availability, Error<F::Error>> {
        if !valid_relative(relative_file) {
            return Err(Error::InvalidRelativePath);
        }
        match fs.volume_id(&self.mount).map_err(Error::Probe)? {
            None => return Ok(Availability::Offline),
            Some(id) if id != self.volume_id => return Err(Error::IdentityMismatch),
            Some(_) => {}
        }
        match fs
            .source_id(&self.mount.join(&self.relative_root))
            .map_err(Error::Probe)?
        {
            None => return Ok(Availability::Missing),
            Some(id) if id != self.source_id => return Err(Error::IdentityMismatch),
            Some(_) => {}
        }
        Ok(
            if fs
                .file_exists(&self.mount.join(&self.relative_root).join(relative_file))
                .map_err(Error::Probe)?
            {
                Availability::Online
            } else {
                Availability::Missing
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    #[derive(Default)]
    struct Fake {
        volumes: HashMap<PathBuf, String>,
        sources: HashMap<PathBuf, String>,
        files: HashSet<PathBuf>,
    }
    impl FileSystem for Fake {
        type Error = &'static str;
        fn volume_id(&self, p: &Path) -> Result<Option<String>, Self::Error> {
            Ok(self.volumes.get(p).cloned())
        }
        fn source_id(&self, p: &Path) -> Result<Option<String>, Self::Error> {
            Ok(self.sources.get(p).cloned())
        }
        fn file_exists(&self, p: &Path) -> Result<bool, Self::Error> {
            Ok(self.files.contains(p))
        }
    }
    #[test]
    fn disconnected_relocated_and_missing_preserve_identity_and_decision() {
        let mut fs = Fake::default();
        let mut source = Source::new::<&str>(
            "disk-1".into(),
            "source-1".into(),
            "/old".into(),
            "photos".into(),
        )
        .unwrap();
        let photo_id = 42;
        let decision = "KEEP";
        assert_eq!(
            source.file_status(&fs, Path::new("a.raw")),
            Ok(Availability::Offline)
        );
        fs.volumes.insert("/new".into(), "disk-1".into());
        fs.sources.insert("/new/photos".into(), "source-1".into());
        fs.files.insert("/new/photos/a.raw".into());
        source.relocate(&fs, "/new".into()).unwrap();
        assert_eq!(
            source.file_status(&fs, Path::new("a.raw")),
            Ok(Availability::Online)
        );
        assert_eq!(
            source.file_status(&fs, Path::new("gone.raw")),
            Ok(Availability::Missing)
        );
        assert_eq!(
            (photo_id, decision, &source.relative_root),
            (42, "KEEP", &PathBuf::from("photos"))
        );
        fs.volumes.insert("/wrong".into(), "disk-2".into());
        assert_eq!(
            source.relocate(&fs, "/wrong".into()),
            Err(Error::IdentityMismatch)
        );
        assert_eq!(source.mount, PathBuf::from("/new"));
        assert_eq!(
            source.file_status(&fs, Path::new("../secret")),
            Err(Error::InvalidRelativePath)
        );
    }
}
