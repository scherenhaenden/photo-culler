//! Source-relative import discovery and preflight. Both use the same traversal.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Role {
    Raw,
    Jpeg,
    Image,
    Sidecar,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Source-relative path; absolute server paths stay inside the scanner.
    pub path: PathBuf,
    pub role: Role,
    pub bytes: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Estimate {
    pub entries: Vec<Entry>,
    pub total_files: usize,
    pub total_bytes: u64,
    pub logical_photos: usize,
    pub extensions: BTreeMap<String, usize>,
    pub roles: BTreeMap<Role, usize>,
}
#[derive(Debug, Clone)]
pub struct Options {
    pub recursive: bool,
    pub exclude_patterns: Vec<String>,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            recursive: true,
            exclude_patterns: vec![],
        }
    }
}

fn role(ext: &str) -> Option<Role> {
    match ext {
        "3fr" | "arw" | "cr2" | "cr3" | "dng" | "iiq" | "mos" | "nef" | "nrw" | "orf" | "pef"
        | "raf" | "rw2" | "rwl" | "srw" => Some(Role::Raw),
        "jpg" | "jpeg" => Some(Role::Jpeg),
        "heic" | "heif" | "png" | "tif" | "tiff" => Some(Role::Image),
        "xmp" | "pp3" | "dop" => Some(Role::Sidecar),
        _ => None,
    }
}
fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
}

// Python fnmatchcase semantics: '*' and '?' also match '/', and brackets form classes.
fn glob(pattern: &[char], value: &[char]) -> bool {
    if pattern.is_empty() {
        return value.is_empty();
    }
    match pattern[0] {
        '*' => glob(&pattern[1..], value) || (!value.is_empty() && glob(pattern, &value[1..])),
        '?' => !value.is_empty() && glob(&pattern[1..], &value[1..]),
        '[' => {
            let mut end = 1;
            if pattern.get(end) == Some(&'!') {
                end += 1;
            }
            if pattern.get(end) == Some(&']') {
                end += 1;
            }
            while end < pattern.len() && pattern[end] != ']' {
                end += 1;
            }
            if end == pattern.len() {
                return !value.is_empty() && value[0] == '[' && glob(&pattern[1..], &value[1..]);
            }
            if value.is_empty() {
                return false;
            }
            let negate = pattern.get(1) == Some(&'!');
            let mut i = if negate { 2 } else { 1 };
            let mut found = false;
            while i < end {
                if i + 2 < end && pattern[i + 1] == '-' {
                    found |= pattern[i] <= value[0] && value[0] <= pattern[i + 2];
                    i += 3;
                } else {
                    found |= pattern[i] == value[0];
                    i += 1;
                }
            }
            (found != negate) && glob(&pattern[end + 1..], &value[1..])
        }
        c => !value.is_empty() && c == value[0] && glob(&pattern[1..], &value[1..]),
    }
}
fn excluded(relative: &Path, patterns: &[String]) -> bool {
    let candidate = relative.to_string_lossy().replace('\\', "/");
    let name = relative.file_name().unwrap_or_default().to_string_lossy();
    patterns.iter().any(|p| {
        let pat: Vec<char> = p.chars().collect();
        glob(&pat, &candidate.chars().collect::<Vec<_>>())
            || glob(&pat, &name.chars().collect::<Vec<_>>())
    })
}
/// `source` must resolve within one of the explicitly authorized directory roots.
/// Symlink entries, including directory links, are never followed. IO errors are returned,
/// so an incomplete traversal cannot be mistaken for a complete estimate.
pub fn scan(
    source: &Path,
    authorized_roots: &[PathBuf],
    options: &Options,
) -> io::Result<Vec<Entry>> {
    let source = fs::canonicalize(source)?;
    if !source.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source must be a directory",
        ));
    }
    let mut allowed = false;
    for root in authorized_roots {
        let root = fs::canonicalize(root)?;
        if root.is_dir() && source.starts_with(root) {
            allowed = true;
        }
    }
    if !allowed {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "source outside authorized roots",
        ));
    }
    let patterns: Vec<String> = options
        .exclude_patterns
        .iter()
        .map(|p| p.trim().replace('\\', "/"))
        .filter(|p| !p.is_empty())
        .collect();
    if patterns.len() > 50 || patterns.iter().any(|p| p.chars().count() > 256) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid exclusion patterns",
        ));
    }
    let mut entries = Vec::new();
    let mut dirs = vec![source.clone()];
    while let Some(dir) = dirs.pop() {
        for item in fs::read_dir(dir)? {
            let item = item?;
            let path = item.path();
            let name = item.file_name();
            let name = name.to_string_lossy();
            let ty = item.file_type()?;
            if ty.is_symlink()
                || name.starts_with('.')
                || excluded(path.strip_prefix(&source).unwrap(), &patterns)
            {
                continue;
            }
            if ty.is_dir() {
                if options.recursive
                    && !matches!(name.to_lowercase().as_str(), "__pycache__" | "@eadir")
                {
                    dirs.push(path);
                }
            } else if ty.is_file() {
                if let Some(role) = role(&extension(&path)) {
                    entries.push(Entry {
                        bytes: item.metadata()?.len(),
                        path: path
                            .strip_prefix(&source)
                            .expect("walked entry remains under source")
                            .to_path_buf(),
                        role,
                    });
                }
            }
        }
    }
    entries.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(entries)
}
pub fn preflight(
    source: &Path,
    authorized_roots: &[PathBuf],
    options: &Options,
) -> io::Result<Estimate> {
    let entries = scan(source, authorized_roots, options)?;
    let mut extensions = BTreeMap::new();
    let mut roles = BTreeMap::new();
    let mut groups = BTreeSet::new();
    let mut total_bytes = 0u64;
    for entry in &entries {
        total_bytes = total_bytes
            .checked_add(entry.bytes)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "byte count overflow"))?;
        *extensions
            .entry(format!(".{}", extension(&entry.path)))
            .or_insert(0) += 1;
        *roles.entry(entry.role).or_insert(0) += 1;
        let stem = entry.path.file_stem().unwrap_or_default();
        let stem_path = Path::new(stem);
        let logical = if matches!(
            role(&extension(stem_path)),
            Some(Role::Raw | Role::Jpeg | Role::Image)
        ) {
            stem_path.file_stem().unwrap_or(stem)
        } else {
            stem
        };
        groups.insert(
            entry
                .path
                .parent()
                .unwrap_or(Path::new(""))
                .join(logical)
                .to_string_lossy()
                .to_lowercase(),
        );
    }
    Ok(Estimate {
        total_files: entries.len(),
        total_bytes,
        logical_photos: groups.len(),
        entries,
        extensions,
        roles,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    #[test]
    fn synthetic_tree_preflight_matches_scan_and_respects_boundaries() {
        let base = std::env::temp_dir().join(format!(
            "photo-culler-scan-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let source = base.join("source");
        let outside = base.join("outside");
        fs::create_dir_all(source.join("nested/previews")).unwrap();
        fs::create_dir_all(&outside).unwrap();
        for (path, data) in [
            ("A.NEF", "raw"),
            ("A.JPG", "jpeg"),
            ("A.NEF.xmp", "x"),
            ("nested/B.png", "png"),
            ("nested/previews/skip.jpg", "skip"),
            ("nested/skip1.jpg", "x"),
            (".hidden.jpg", "x"),
            ("ignored.txt", "x"),
        ] {
            fs::write(source.join(path), data).unwrap();
        }
        fs::write(outside.join("secret.jpg"), "secret").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(outside.join("secret.jpg"), source.join("link.jpg"))
                .unwrap();
            std::os::unix::fs::symlink(&outside, source.join("linkdir")).unwrap();
        }
        let roots = vec![source.clone()];
        let options = Options {
            recursive: true,
            exclude_patterns: vec!["nested/previews/**".into(), "skip[0-9].jpg".into()],
        };
        let scanned = scan(&source, &roots, &options).unwrap();
        let estimate = preflight(&source, &roots, &options).unwrap();
        assert_eq!(estimate.entries, scanned);
        assert!(scanned.iter().all(|entry| !entry.path.is_absolute()));
        assert_eq!(estimate.total_files, 4);
        assert_eq!(estimate.total_bytes, 11);
        assert_eq!(estimate.logical_photos, 2);
        assert_eq!(estimate.extensions.get(".nef"), Some(&1));
        assert_eq!(
            scan(&outside, &roots, &options).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            scan(
                &source,
                &roots,
                &Options {
                    recursive: false,
                    ..options
                }
            )
            .unwrap()
            .len(),
            3
        );
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn fnmatch_works_on_path_and_classes() {
        assert!(glob(
            &"nested/**".chars().collect::<Vec<_>>(),
            &"nested/a/b".chars().collect::<Vec<_>>()
        ));
        assert!(glob(
            &"[!a-c]?".chars().collect::<Vec<_>>(),
            &"dz".chars().collect::<Vec<_>>()
        ));
    }
}
