//! Source-relative import discovery and preflight. Both use the same traversal.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs, io,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
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
    pub modified_ns: Option<u128>,
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
        "3fr" | "arw" | "cr2" | "cr3" | "dng" | "fff" | "iiq" | "kdc" | "mef" | "mos" | "mrw"
        | "nef" | "nrw" | "orf" | "pef" | "raf" | "raw" | "rw2" | "rwl" | "srw" => Some(Role::Raw),
        "jpg" | "jpeg" | "jpe" => Some(Role::Jpeg),
        "bmp" | "gif" | "heic" | "heif" | "png" | "tif" | "tiff" | "webp" => Some(Role::Image),
        "aae" | "dop" | "pp3" | "xmp" => Some(Role::Sidecar),
        _ => None,
    }
}
fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
}

pub fn role_for_path(path: &Path) -> Option<Role> {
    role(&extension(path))
}

// Python fnmatchcase semantics: '*' and '?' also match '/', and brackets form classes.
fn glob(pattern: &[char], value: &[char]) -> bool {
    enum Token {
        Star,
        Any,
        Literal(char),
        Class {
            start: usize,
            end: usize,
            negate: bool,
        },
    }

    let mut tokens = Vec::new();
    let mut p = 0;
    while p < pattern.len() {
        match pattern[p] {
            '*' => {
                tokens.push(Token::Star);
                p += 1;
            }
            '?' => {
                tokens.push(Token::Any);
                p += 1;
            }
            '[' => {
                let mut end = p + 1;
                let negate = pattern.get(end) == Some(&'!');
                if negate {
                    end += 1;
                }
                if pattern.get(end) == Some(&']') {
                    end += 1;
                }
                while end < pattern.len() && pattern[end] != ']' {
                    end += 1;
                }
                if end == pattern.len() {
                    tokens.push(Token::Literal('['));
                    p += 1;
                } else {
                    tokens.push(Token::Class {
                        start: p + if negate { 2 } else { 1 },
                        end,
                        negate,
                    });
                    p = end + 1;
                }
            }
            c => {
                tokens.push(Token::Literal(c));
                p += 1;
            }
        }
    }

    // Each row represents the remaining pattern at every value position.
    // A star also depends on the current row at the next value position.
    let mut next = vec![false; value.len() + 1];
    next[value.len()] = true;
    let mut current = vec![false; value.len() + 1];
    for token in tokens.iter().rev() {
        current[value.len()] = matches!(token, Token::Star) && next[value.len()];
        for v in (0..value.len()).rev() {
            current[v] = match token {
                Token::Star => next[v] || current[v + 1],
                Token::Any => next[v + 1],
                Token::Literal(c) => *c == value[v] && next[v + 1],
                Token::Class { start, end, negate } => {
                    let mut i = *start;
                    let mut found = false;
                    while i < *end {
                        if i + 2 < *end && pattern[i + 1] == '-' {
                            found |= pattern[i] <= value[v] && value[v] <= pattern[i + 2];
                            i += 3;
                        } else {
                            found |= pattern[i] == value[v];
                            i += 1;
                        }
                    }
                    (found != *negate) && next[v + 1]
                }
            };
        }
        std::mem::swap(&mut current, &mut next);
    }
    next[0]
}

pub(crate) fn excluded(relative: &Path, patterns: &[String]) -> bool {
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
                    let metadata = item.metadata()?;
                    entries.push(Entry {
                        bytes: metadata.len(),
                        modified_ns: modified_ns(&metadata),
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

/// Traverse supported files in filesystem order and emit each path immediately.
///
/// Unlike [`scan`] and [`scan_after`], this does not collect or sort directory
/// entries. The callback controls file buffering. Pending directories are
/// stored as paths, so traversal holds only one `ReadDir` at a time; memory
/// for pending paths can grow with the number of directories in the tree.
pub fn walk<F, P>(
    source: &Path,
    authorized_roots: &[PathBuf],
    options: &Options,
    mut visit: F,
    mut progress: P,
) -> io::Result<()>
where
    F: FnMut(Entry) -> io::Result<()>,
    P: FnMut() -> io::Result<()>,
{
    let source = fs::canonicalize(source)?;
    if !source.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source must be a directory",
        ));
    }
    if !is_authorized(&source, authorized_roots)? {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "source outside authorized roots",
        ));
    }
    let patterns = normalized_patterns(options)?;

    let mut stack = vec![source.clone()];
    let mut visited_entries = 0usize;
    while let Some(directory) = stack.pop() {
        // A queued directory may have changed since it was discovered. Reject
        // a replacement symlink or a path that now resolves outside the root.
        if fs::symlink_metadata(&directory)?.file_type().is_symlink() {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "source directory was replaced by a symlink",
            ));
        }
        let resolved = fs::canonicalize(&directory)?;
        if !resolved.starts_with(&source) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "source directory resolves outside its authorized root",
            ));
        }
        for next_item in fs::read_dir(resolved)? {
            let item = next_item?;
            visited_entries += 1;
            if visited_entries % 256 == 0 {
                progress()?;
            }
            let path = item.path();
            let relative = path
                .strip_prefix(&source)
                .expect("walked entry remains under source")
                .to_path_buf();
            let name = item.file_name();
            let name = name.to_string_lossy();
            let kind = item.file_type()?;
            if kind.is_symlink() || name.starts_with('.') || excluded(&relative, &patterns) {
                continue;
            }
            if kind.is_dir() {
                if options.recursive
                    && !matches!(name.to_lowercase().as_str(), "__pycache__" | "@eadir")
                {
                    let resolved = fs::canonicalize(&path)?;
                    if !resolved.starts_with(&source) {
                        return Err(io::Error::new(
                            io::ErrorKind::PermissionDenied,
                            "source directory resolves outside its authorized root",
                        ));
                    }
                    stack.push(resolved);
                }
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            let Some(role) = role(&extension(&relative)) else {
                continue;
            };
            let metadata = item.metadata()?;
            visit(Entry {
                path: relative,
                role,
                bytes: metadata.len(),
                modified_ns: modified_ns(&metadata),
            })?;
        }
    }
    Ok(())
}

fn is_authorized(source: &Path, authorized_roots: &[PathBuf]) -> io::Result<bool> {
    for root in authorized_roots {
        let root = fs::canonicalize(root)?;
        if root.is_dir() && source.starts_with(root) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn normalized_patterns(options: &Options) -> io::Result<Vec<String>> {
    let patterns: Vec<String> = options
        .exclude_patterns
        .iter()
        .map(|pattern| pattern.trim().replace('\\', "/"))
        .filter(|pattern| !pattern.is_empty())
        .collect();
    if patterns.len() > 50 || patterns.iter().any(|pattern| pattern.chars().count() > 256) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid exclusion patterns",
        ));
    }
    Ok(patterns)
}

fn modified_ns(metadata: &fs::Metadata) -> Option<u128> {
    metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_nanos())
}

/// Return at most `limit` eligible files strictly after `after` in cursor order.
///
/// Cursor paths always use `/` separators, including on Windows, so traversal,
/// estimate pagination, and persisted worker cursors use one lexical ordering.
///
/// Unlike [`scan`], this stops as soon as the page is full. Each visited
/// directory is still read and sorted for every page, so a large flat directory
/// has memory proportional to its entry count and repeated pages rescan it.
pub fn scan_after(
    source: &Path,
    authorized_roots: &[PathBuf],
    options: &Options,
    after: Option<&str>,
    limit: usize,
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

    if limit == 0 {
        return Ok(Vec::new());
    }

    // A directory's key includes its trailing slash. That makes the key sort in
    // the same position as all paths below it (for example, `a.txt` sorts before
    // `a/file.jpg`). Processing sorted children depth-first is therefore the
    // same global lexicographic order used by the cursor.
    struct Child {
        entry: fs::DirEntry,
        absolute: PathBuf,
        relative: PathBuf,
        key: String,
        is_dir: bool,
    }
    fn children(dir: &Path, source: &Path, patterns: &[String]) -> io::Result<Vec<Child>> {
        let mut items = Vec::new();
        for item in fs::read_dir(dir)? {
            let item = item?;
            let absolute = item.path();
            let name = item.file_name();
            let name = name.to_string_lossy();
            let ty = item.file_type()?;
            if ty.is_symlink() || name.starts_with('.') {
                continue;
            }
            let relative = absolute
                .strip_prefix(source)
                .expect("walked entry remains under source")
                .to_path_buf();
            if excluded(&relative, patterns) {
                continue;
            }
            let is_dir = ty.is_dir();
            if !is_dir && !ty.is_file() {
                continue;
            }
            let mut key = portable_path_key(&relative);
            if is_dir {
                key.push('/');
            }
            items.push(Child {
                entry: item,
                absolute,
                relative,
                key,
                is_dir,
            });
        }
        items.sort_by(|a, b| a.key.cmp(&b.key));
        Ok(items)
    }

    let mut result = Vec::with_capacity(limit.min(256));
    let mut stack: Vec<std::vec::IntoIter<Child>> =
        vec![children(&source, &source, &patterns)?.into_iter()];
    while !stack.is_empty() && result.len() < limit {
        let next = stack.last_mut().and_then(|iter| iter.next());
        let Some(child) = next else {
            stack.pop();
            continue;
        };

        if child.is_dir {
            if !options.recursive
                || matches!(
                    child
                        .absolute
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_lowercase()
                        .as_str(),
                    "__pycache__" | "@eadir"
                )
            {
                continue;
            }
            // If the cursor is inside this directory, descend to find its later
            // entries. If its whole subtree sorts before the cursor, skip it.
            if after.map_or(false, |cursor| {
                child.key.as_str() <= cursor && !cursor.starts_with(&child.key)
            }) {
                continue;
            }
            stack.push(children(&child.absolute, &source, &patterns)?.into_iter());
            continue;
        }

        let key = portable_path_key(&child.relative);
        if after.map_or(false, |cursor| key.as_str() <= cursor) {
            continue;
        }
        if let Some(role) = role(&extension(&child.relative)) {
            let metadata = child.entry.metadata()?;
            result.push(Entry {
                bytes: metadata.len(),
                modified_ns: modified_ns(&metadata),
                path: child.relative,
                role,
            });
        }
    }
    Ok(result)
}

fn portable_path_key(path: &Path) -> String {
    let value = path.to_string_lossy();
    if std::path::MAIN_SEPARATOR == '\\' {
        value.replace('\\', "/")
    } else {
        value.into_owned()
    }
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
