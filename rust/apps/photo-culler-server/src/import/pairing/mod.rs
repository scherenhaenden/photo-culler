//! Deterministic logical association of image files and metadata sidecars.
//!
//! This module deliberately does not create or derive photo IDs. Callers keep
//! existing catalogue identity and use these groups only to associate file
//! records during import or rescan.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// A file record supplied by the scanner. `relative_path` is relative to the
/// authorized source root, so equal names under different directories remain
/// distinct.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PairingFile {
    pub id: String,
    pub relative_path: PathBuf,
}

/// How media records in a logical filename group can safely be associated.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Association {
    /// One media file, with no competing candidate of its representation.
    Single,
    /// Exactly one RAW and one JPEG candidate.
    RawJpegPair,
    /// More than one candidate of the same representation exists. No
    /// arbitrary candidate is selected; all records remain visible together
    /// for caller review.
    Ambiguous,
}

/// A path-aware photo grouping. Sidecars attach only when their logical stem
/// resolves to exactly one media group in the same directory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PhotoAssociation {
    pub key: String,
    pub media: Vec<PairingFile>,
    pub sidecars: Vec<PairingFile>,
    pub association: Association,
}

/// A sidecar that did not resolve to exactly one media group.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnattachedSidecar {
    pub file: PairingFile,
    pub reason: SidecarReason,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SidecarReason {
    NoMediaCandidate,
    AmbiguousMediaCandidates,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PairingResult {
    pub photos: Vec<PhotoAssociation>,
    pub unattached_sidecars: Vec<UnattachedSidecar>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum FileKind {
    Raw,
    Jpeg,
    OtherImage,
    Sidecar,
}

/// Associate RAW, JPEG, other supported raster images, and XMP/AAE sidecars.
///
/// Matching is deterministic and uses the lower-cased relative parent path
/// plus logical filename stem. `name.ext.xmp` and `name.xmp` both resolve to
/// logical stem `name`. RAW/JPEG candidates pair only when the group contains
/// at most one candidate of each kind. Duplicate same-kind candidates make
/// the entire media group ambiguous; no extension or input-order preference
/// is used. Sidecars attach only to a unique media group. An unattached or
/// ambiguous sidecar stays in `unattached_sidecars` rather than being dropped.
pub fn associate(files: impl IntoIterator<Item = PairingFile>) -> PairingResult {
    let mut media: BTreeMap<String, Vec<PairingFile>> = BTreeMap::new();
    let mut sidecars: BTreeMap<String, Vec<PairingFile>> = BTreeMap::new();

    for file in files {
        let Some(kind) = classify(&file.relative_path) else {
            continue;
        };
        let key = logical_key(&file.relative_path, kind);
        match kind {
            FileKind::Sidecar => sidecars.entry(key).or_default().push(file),
            _ => media.entry(key).or_default().push(file),
        }
    }

    for records in media.values_mut() {
        records.sort_by(|a, b| a.relative_path.cmp(&b.relative_path).then(a.id.cmp(&b.id)));
    }
    for records in sidecars.values_mut() {
        records.sort_by(|a, b| a.relative_path.cmp(&b.relative_path).then(a.id.cmp(&b.id)));
    }

    let mut result = PairingResult::default();
    for (key, records) in media {
        let raw_count = records
            .iter()
            .filter(|f| classify(&f.relative_path) == Some(FileKind::Raw))
            .count();
        let jpeg_count = records
            .iter()
            .filter(|f| classify(&f.relative_path) == Some(FileKind::Jpeg))
            .count();
        let other_count = records
            .iter()
            .filter(|f| classify(&f.relative_path) == Some(FileKind::OtherImage))
            .count();
        let association = if raw_count > 1 || jpeg_count > 1 || other_count > 1 {
            Association::Ambiguous
        } else if raw_count == 1 && jpeg_count == 1 && other_count == 0 {
            Association::RawJpegPair
        } else {
            Association::Single
        };
        let mut attached = sidecars.remove(&key).unwrap_or_default();
        if association == Association::Ambiguous {
            result
                .unattached_sidecars
                .extend(attached.drain(..).map(|file| UnattachedSidecar {
                    file,
                    reason: SidecarReason::AmbiguousMediaCandidates,
                }));
        }
        result.photos.push(PhotoAssociation {
            key,
            media: records,
            sidecars: if association == Association::Ambiguous {
                Vec::new()
            } else {
                attached
            },
            association,
        });
    }

    for (key, records) in sidecars {
        let has_ambiguous_media = result
            .photos
            .iter()
            .any(|p| p.key == key && p.association == Association::Ambiguous);
        let reason = if has_ambiguous_media {
            SidecarReason::AmbiguousMediaCandidates
        } else {
            SidecarReason::NoMediaCandidate
        };
        result.unattached_sidecars.extend(
            records
                .into_iter()
                .map(|file| UnattachedSidecar { file, reason }),
        );
    }
    result.photos.sort_by(|a, b| a.key.cmp(&b.key));
    result
        .unattached_sidecars
        .sort_by(|a, b| a.file.relative_path.cmp(&b.file.relative_path));
    result
}

fn classify(path: &Path) -> Option<FileKind> {
    let name = path.file_name()?.to_string_lossy();
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".xmp") || lower.ends_with(".aae") {
        return Some(FileKind::Sidecar);
    }
    let extension = path.extension()?.to_string_lossy().to_ascii_lowercase();
    match extension.as_str() {
        "jpg" | "jpeg" | "jpe" => Some(FileKind::Jpeg),
        "arw" | "cr2" | "cr3" | "dng" | "nef" | "nrw" | "orf" | "pef" | "raf" | "raw" | "rw2"
        | "srw" | "3fr" | "fff" | "iiq" | "kdc" | "mos" | "mef" | "mrw" => Some(FileKind::Raw),
        "png" | "tif" | "tiff" | "bmp" | "gif" | "webp" | "heic" | "heif" => {
            Some(FileKind::OtherImage)
        }
        _ => None,
    }
}

fn logical_key(path: &Path, kind: FileKind) -> String {
    let mut parent = PathBuf::new();
    for component in path.parent().unwrap_or_else(|| Path::new("")).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                parent.push("..");
            }
            Component::Normal(part) => parent.push(part),
            Component::RootDir | Component::Prefix(_) => parent.push(component.as_os_str()),
        }
    }
    let mut stem = path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    if kind == FileKind::Sidecar {
        stem = strip_last_extension(&stem);
        if extension_of(&stem)
            .as_deref()
            .is_some_and(is_media_extension)
        {
            stem = strip_last_extension(&stem);
        }
    } else {
        stem = strip_last_extension(&stem);
    }
    format!(
        "{}/{}",
        parent.to_string_lossy().to_lowercase(),
        stem.to_lowercase()
    )
}

fn strip_last_extension(name: &str) -> String {
    match name.rfind('.') {
        Some(index) if index > 0 => name[..index].to_owned(),
        _ => name.to_owned(),
    }
}

fn extension_of(name: &str) -> Option<String> {
    name.rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
}

fn is_media_extension(extension: &str) -> bool {
    matches!(
        extension,
        "jpg"
            | "jpeg"
            | "jpe"
            | "arw"
            | "cr2"
            | "cr3"
            | "dng"
            | "nef"
            | "nrw"
            | "orf"
            | "pef"
            | "raf"
            | "raw"
            | "rw2"
            | "srw"
            | "3fr"
            | "fff"
            | "iiq"
            | "kdc"
            | "mos"
            | "mef"
            | "mrw"
            | "png"
            | "tif"
            | "tiff"
            | "bmp"
            | "gif"
            | "webp"
            | "heic"
            | "heif"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: &str, path: &str) -> PairingFile {
        PairingFile {
            id: id.into(),
            relative_path: path.into(),
        }
    }

    #[test]
    fn pairs_raw_jpeg_and_compound_sidecar() {
        let result = associate([
            file("raw", "shoot/DSC_001.ARW"),
            file("jpeg", "shoot/DSC_001.JPG"),
            file("xmp", "shoot/DSC_001.JPG.xmp"),
        ]);
        assert_eq!(result.photos.len(), 1);
        let photo = &result.photos[0];
        assert_eq!(photo.association, Association::RawJpegPair);
        assert_eq!(
            photo
                .media
                .iter()
                .map(|f| f.id.as_str())
                .collect::<Vec<_>>(),
            ["raw", "jpeg"]
        );
        assert_eq!(
            photo
                .sidecars
                .iter()
                .map(|f| f.id.as_str())
                .collect::<Vec<_>>(),
            ["xmp"]
        );
        assert!(result.unattached_sidecars.is_empty());
    }

    #[test]
    fn same_basename_in_different_directories_stays_separate() {
        let result = associate([
            file("a-raw", "day-a/IMG_1.CR2"),
            file("a-jpeg", "day-a/IMG_1.JPG"),
            file("b-jpeg", "day-b/IMG_1.JPG"),
        ]);
        assert_eq!(result.photos.len(), 2);
        assert_eq!(result.photos[0].association, Association::RawJpegPair);
        assert_eq!(result.photos[1].association, Association::Single);
    }

    #[test]
    fn duplicate_raw_candidates_are_ambiguous_and_sidecar_is_not_guessed() {
        let result = associate([
            file("raw-a", "shoot/IMG_2.CR2"),
            file("raw-b", "shoot/IMG_2.NEF"),
            file("jpeg", "shoot/IMG_2.JPG"),
            file("xmp", "shoot/IMG_2.XMP"),
        ]);
        assert_eq!(result.photos.len(), 1);
        assert_eq!(result.photos[0].association, Association::Ambiguous);
        assert!(result.photos[0].sidecars.is_empty());
        assert_eq!(result.unattached_sidecars.len(), 1);
        assert_eq!(
            result.unattached_sidecars[0].reason,
            SidecarReason::AmbiguousMediaCandidates
        );
    }

    #[test]
    fn extensionless_sidecar_stays_unattached_if_media_is_missing() {
        let result = associate([file("sidecar", "shoot/IMG_3.xmp")]);
        assert!(result.photos.is_empty());
        assert_eq!(
            result.unattached_sidecars[0].reason,
            SidecarReason::NoMediaCandidate
        );
    }

    #[test]
    fn pairing_does_not_generate_or_change_photo_identity() {
        let input_id = "existing-file-id";
        let result = associate([file(input_id, "shoot/IMG_4.JPG")]);
        assert_eq!(result.photos[0].media[0].id, input_id);
    }
}
