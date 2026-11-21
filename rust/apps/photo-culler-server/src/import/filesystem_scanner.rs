//! Bounded filesystem adapter for the resumable import worker.
//!
//! Each source is injected by an opaque ID together with a server-local root.
//! HTTP request data never participates in filesystem path construction.

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{self, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
    time::UNIX_EPOCH,
};

use crate::import::{
    identity::{DEFAULT_QUICK_HASH_CHUNK_SIZE, compute_full_hash, compute_quick_hash},
    scanning::{self, Options},
    worker::{ManifestEntry, Observation, Scanner, WorkerFuture},
};

/// Largest page of observations this adapter will hash in one worker step.
pub const MAX_OBSERVATIONS_PER_PAGE: usize = 1024;

/// Trusted local configuration for one source.
#[derive(Clone, Debug)]
pub struct SourceConfig {
    /// Canonicalized by [`FilesystemScanner::new`].
    pub root: PathBuf,
    pub options: Options,
}

/// Reads source files without writing or renaming originals.
///
/// The allowed roots are configured by the local application/runtime. A source
/// is accepted only if its canonical path lies beneath one of those roots.
/// Symlink entries are rejected during traversal and each opened file is
/// canonicalized and checked again. The portable `std::fs` API cannot pin a
/// no-follow directory-relative handle on every supported OS, so a hostile
/// process that can mutate the source tree concurrently could still race the
/// final canonicalize/open pair. This adapter assumes source directories are
/// not adversarially rewritten during a scan; size/mtime changes are rejected.
#[derive(Clone, Debug)]
pub struct FilesystemScanner {
    sources: BTreeMap<String, SourceConfig>,
    authorized_roots: Vec<PathBuf>,
}

impl FilesystemScanner {
    /// Build an adapter from trusted, server-local source configuration.
    pub fn new(
        sources: impl IntoIterator<Item = (String, SourceConfig)>,
        authorized_roots: impl IntoIterator<Item = PathBuf>,
    ) -> Result<Self, String> {
        let mut roots = Vec::new();
        for root in authorized_roots {
            let root = fs::canonicalize(&root)
                .map_err(|error| format!("could not resolve authorized root: {error}"))?;
            if !root.is_dir() {
                return Err("authorized root is not a directory".into());
            }
            roots.push(root);
        }
        if roots.is_empty() {
            return Err("at least one authorized root is required".into());
        }
        roots.sort();
        roots.dedup();
        for (index, first) in roots.iter().enumerate() {
            for second in roots.iter().skip(index + 1) {
                if first.starts_with(second) || second.starts_with(first) {
                    return Err(format!(
                        "authorized roots overlap (`{}` and `{}`); configure non-overlapping roots",
                        first.display(),
                        second.display()
                    ));
                }
            }
        }

        let mut configured = BTreeMap::new();
        for (source_id, mut source) in sources {
            validate_source_id(&source_id)?;
            let root = fs::canonicalize(&source.root)
                .map_err(|error| format!("could not resolve source `{source_id}`: {error}"))?;
            if !root.is_dir() || !roots.iter().any(|allowed| root.starts_with(allowed)) {
                return Err(format!(
                    "source `{source_id}` is outside the configured authorized roots"
                ));
            }
            source.root = root;
            if configured.insert(source_id.clone(), source).is_some() {
                return Err(format!("duplicate import source ID `{source_id}`"));
            }
        }

        Ok(Self {
            sources: configured,
            authorized_roots: roots,
        })
    }
}

impl Scanner for FilesystemScanner {
    fn build_manifest<'a>(
        &'a mut self,
        source_id: &'a str,
        batches: tokio::sync::mpsc::Sender<Vec<ManifestEntry>>,
    ) -> WorkerFuture<'a, Result<(), String>> {
        let source = self.sources.get(source_id).cloned();
        let source_id = source_id.to_owned();
        let authorized_roots = self.authorized_roots.clone();
        Box::pin(async move {
            let source = source.ok_or_else(|| format!("unknown import source `{source_id}`"))?;
            tokio::task::spawn_blocking(move || {
                let (root, options) = (source.root, source.options);
                let mut batch = Vec::with_capacity(MANIFEST_BATCH_SIZE);
                scanning::walk(
                    &root,
                    &authorized_roots,
                    &options,
                    |entry| {
                        let normalized = normalize_relative_path(&entry.path)
                            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                        batch.push(ManifestEntry {
                            path: portable_path_text(&normalized),
                            size: entry.bytes,
                            modified_ns: entry.modified_ns,
                            role: role_name(entry.role).to_owned(),
                        });
                        if batch.len() >= MANIFEST_BATCH_SIZE {
                            batches
                                .blocking_send(std::mem::replace(
                                    &mut batch,
                                    Vec::with_capacity(MANIFEST_BATCH_SIZE),
                                ))
                                .map_err(|_| {
                                    io::Error::new(
                                        io::ErrorKind::BrokenPipe,
                                        "manifest receiver closed before traversal completed",
                                    )
                                })?;
                        }
                        Ok(())
                    },
                    || {
                        batches.blocking_send(Vec::new()).map_err(|_| {
                            io::Error::new(
                                io::ErrorKind::BrokenPipe,
                                "manifest receiver closed before traversal completed",
                            )
                        })
                    },
                )
                .map_err(|error| format!("source traversal failed: {error}"))?;
                if !batch.is_empty() {
                    batches
                        .blocking_send(batch)
                        .map_err(|_| "manifest receiver closed before traversal completed")?;
                }
                Ok(())
            })
            .await
            .map_err(|error| format!("filesystem scanner task failed: {error}"))?
        })
    }

    fn observe_page<'a>(
        &'a mut self,
        source_id: &'a str,
        entries: &'a [ManifestEntry],
    ) -> WorkerFuture<'a, Result<Vec<Observation>, String>> {
        let source = self.sources.get(source_id).cloned();
        let source_id = source_id.to_owned();
        let entries = entries.to_vec();
        Box::pin(async move {
            let source = source.ok_or_else(|| format!("unknown import source `{source_id}`"))?;
            if entries.len() > MAX_OBSERVATIONS_PER_PAGE {
                return Err("manifest page exceeds the filesystem observation limit".into());
            }
            tokio::task::spawn_blocking(move || observe_manifest_page(source, entries))
                .await
                .map_err(|error| format!("filesystem observer task failed: {error}"))?
        })
    }
}

fn observe_manifest_page(
    source: SourceConfig,
    entries: Vec<ManifestEntry>,
) -> Result<Vec<Observation>, String> {
    let mut observations = Vec::with_capacity(entries.len());
    for entry in entries {
        let relative_native = PathBuf::from(&entry.path);
        let normalized = normalize_relative_path(&relative_native)?;
        if portable_path_text(&normalized) != entry.path {
            return Err("manifest path is not in canonical portable form".into());
        }
        let expected_role = scanning::role_for_path(&normalized)
            .ok_or_else(|| "manifest contains an unsupported file type".to_owned())?;
        if role_name(expected_role) != entry.role {
            return Err("manifest file role does not match its path".into());
        }
        let observation = observe_file(
            &source.root,
            &relative_native,
            &normalized,
            entry.size,
            entry.modified_ns,
        )?;
        if observations
            .last()
            .is_some_and(|previous: &Observation| previous.cursor() >= observation.cursor())
        {
            return Err("manifest page is not in strictly increasing path order".into());
        }
        observations.push(observation);
    }
    Ok(observations)
}

const MANIFEST_BATCH_SIZE: usize = 256;

fn role_name(role: scanning::Role) -> &'static str {
    match role {
        scanning::Role::Raw => "RAW",
        scanning::Role::Jpeg => "JPEG",
        scanning::Role::Image => "IMAGE",
        scanning::Role::Sidecar => "SIDECAR",
    }
}

fn portable_path_text(path: &Path) -> String {
    let value = path.to_string_lossy();
    if std::path::MAIN_SEPARATOR == '\\' {
        value.replace('\\', "/")
    } else {
        value.into_owned()
    }
}

fn observe_file(
    root: &Path,
    relative_native: &Path,
    relative_normalized: &Path,
    expected_size: u64,
    expected_modified_ns: Option<u128>,
) -> Result<Observation, String> {
    let candidate = root.join(relative_native);
    let link_metadata = fs::symlink_metadata(&candidate)
        .map_err(|error| format!("could not inspect source file: {error}"))?;
    if link_metadata.file_type().is_symlink() || !link_metadata.is_file() {
        return Err("source entry changed type or became a symlink".into());
    }

    let resolved = fs::canonicalize(&candidate)
        .map_err(|error| format!("could not resolve source file: {error}"))?;
    if !resolved.starts_with(root) {
        return Err("source file resolves outside its authorized root".into());
    }
    let mut file =
        File::open(&resolved).map_err(|error| format!("could not open source file: {error}"))?;
    let before = file
        .metadata()
        .map_err(|error| format!("could not read source file metadata: {error}"))?;
    if !before.is_file()
        || before.len() != expected_size
        || expected_modified_ns.is_some_and(|expected| modified_ns(&before) != Some(expected))
    {
        return Err("source file changed while the directory was being scanned".into());
    }

    let quick_hash = compute_quick_hash(&mut file, before.len(), DEFAULT_QUICK_HASH_CHUNK_SIZE)
        .map_err(|error| format!("could not hash source file header: {error}"))?;
    file.seek(SeekFrom::Start(0))
        .map_err(|error| format!("could not rewind source file: {error}"))?;
    let content_hash = compute_full_hash(&mut file)
        .map_err(|error| format!("could not hash source file: {error}"))?;
    let after = file
        .metadata()
        .map_err(|error| format!("could not recheck source file metadata: {error}"))?;
    if !after.is_file()
        || after.len() != before.len()
        || modified_ns(&after) != modified_ns(&before)
    {
        return Err("source file changed while it was being hashed".into());
    }

    Ok(Observation {
        path: relative_normalized.to_path_buf(),
        size: before.len(),
        modified_ns: modified_ns(&before),
        quick_hash: quick_hash.into_bytes(),
        content_hash: content_hash.into_bytes(),
    })
}

pub(crate) fn normalize_relative_path(path: &Path) -> Result<PathBuf, String> {
    if path.as_os_str().is_empty() || path.is_absolute() {
        return Err("scanner returned an empty or absolute path".into());
    }
    let mut segments = Vec::new();
    for component in path.components() {
        let Component::Normal(segment) = component else {
            return Err("scanner returned a path that escapes its source root".into());
        };
        let segment = segment
            .to_str()
            .ok_or_else(|| "source path contains a non-UTF-8 filename".to_owned())?;
        // Backslashes and colons are separators/stream syntax on some supported
        // platforms; rejecting them keeps the cross-platform cursor unambiguous.
        if segment.is_empty() || segment.contains(['\\', ':', '\0']) {
            return Err("source path contains a non-portable filename component".into());
        }
        segments.push(segment);
    }
    if segments.is_empty() {
        return Err("scanner returned an empty relative path".into());
    }
    Ok(PathBuf::from(segments.join("/")))
}

fn modified_ns(metadata: &fs::Metadata) -> Option<u128> {
    metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_nanos())
}

fn validate_source_id(source_id: &str) -> Result<(), String> {
    if source_id.is_empty()
        || source_id.len() > 128
        || !source_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("invalid opaque import source ID".into());
    }
    Ok(())
}
