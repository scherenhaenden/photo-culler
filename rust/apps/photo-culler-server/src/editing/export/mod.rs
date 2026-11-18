//! Explicit planning and application of derived-file exports.
//!
//! This module has no API that opens or writes an original. Callers supply
//! already-rendered bytes and a destination relative to an approved export
//! root. The writer is required to create new files exclusively.

use std::collections::BTreeSet;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Provenance {
    pub session_id: String,
    pub gallery_id: String,
    pub photo_id: String,
    pub source_name: String,
    pub recipe_revision: Option<String>,
    pub exported_at_utc: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WoAContext {
    pub program: Option<String>,
    pub event_context: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportItem {
    pub provenance: Provenance,
    /// Relative output path under the explicitly approved destination root.
    pub destination: String,
    /// Bytes produced by the renderer; original bytes are never passed here.
    pub derived_bytes: Vec<u8>,
    pub woa: Option<WoAContext>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ItemDisposition {
    Ready,
    Collision,
    NeedsReview(Vec<String>),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedItem {
    pub item: ExportItem,
    pub disposition: ItemDisposition,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExportPlan {
    pub destination_root: String,
    pub items: Vec<PlannedItem>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExportError {
    InvalidDestinationRoot,
    InvalidRelativePath(String),
    DuplicateDestination(String),
    Writer(String),
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ExportReport {
    pub written: Vec<String>,
    pub collisions: Vec<String>,
    pub skipped_for_review: Vec<(String, Vec<String>)>,
    pub provenance: Vec<Provenance>,
}

/// The adapter must implement exclusive creation and must never overwrite.
/// It receives only destination paths and derived bytes, never source paths.
pub trait DerivedWriter {
    fn exists(&self, destination: &str) -> Result<bool, String>;
    fn create_new(&mut self, destination: &str, bytes: &[u8]) -> Result<(), String>;
}

/// Build a reviewable plan without writing anything.
pub fn plan_export<W: DerivedWriter>(
    destination_root: &str,
    items: Vec<ExportItem>,
    writer: &W,
) -> Result<ExportPlan, ExportError> {
    validate_root(destination_root)?;
    let mut seen = BTreeSet::new();
    let mut planned = Vec::with_capacity(items.len());
    for item in items {
        validate_relative_path(&item.destination)?;
        if !seen.insert(item.destination.clone()) {
            return Err(ExportError::DuplicateDestination(item.destination));
        }
        let collision = writer
            .exists(&join(destination_root, &item.destination))
            .map_err(ExportError::Writer)?;
        let review = woa_review_reasons(item.woa.as_ref());
        let disposition = if collision {
            ItemDisposition::Collision
        } else if !review.is_empty() {
            ItemDisposition::NeedsReview(review)
        } else {
            ItemDisposition::Ready
        };
        planned.push(PlannedItem { item, disposition });
    }
    Ok(ExportPlan {
        destination_root: destination_root.to_owned(),
        items: planned,
    })
}

/// Apply only a reviewed plan. Collisions and unresolved editorial context are
/// skipped and reported. Existing files are never replaced.
pub fn apply_plan<W: DerivedWriter>(
    plan: &ExportPlan,
    writer: &mut W,
) -> Result<ExportReport, ExportError> {
    validate_root(&plan.destination_root)?;
    let mut report = ExportReport::default();
    for planned in &plan.items {
        let path = join(&plan.destination_root, &planned.item.destination);
        match &planned.disposition {
            ItemDisposition::Collision => report.collisions.push(path),
            ItemDisposition::NeedsReview(reasons) => {
                report.skipped_for_review.push((path, reasons.clone()));
            }
            ItemDisposition::Ready => {
                // Recheck at apply time to handle races after plan review.
                if writer.exists(&path).map_err(ExportError::Writer)? {
                    report.collisions.push(path);
                    continue;
                }
                writer
                    .create_new(&path, &planned.item.derived_bytes)
                    .map_err(ExportError::Writer)?;
                report.written.push(path);
                report.provenance.push(planned.item.provenance.clone());
            }
        }
    }
    Ok(report)
}

fn woa_review_reasons(context: Option<&WoAContext>) -> Vec<String> {
    let Some(context) = context else {
        return vec!["W:O:A program and event context are missing".into()];
    };
    let mut reasons = Vec::new();
    if context.program.as_deref().map_or(true, str::is_empty) {
        reasons.push("W:O:A program is missing".into());
    }
    if context.event_context.as_deref().map_or(true, str::is_empty) {
        reasons.push("W:O:A event context is missing".into());
    }
    reasons
}

fn validate_root(root: &str) -> Result<(), ExportError> {
    if root.trim().is_empty() || !root.starts_with('/') || root == "/" {
        return Err(ExportError::InvalidDestinationRoot);
    }
    Ok(())
}

fn validate_relative_path(path: &str) -> Result<(), ExportError> {
    let invalid = path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..");
    if invalid {
        Err(ExportError::InvalidRelativePath(path.to_owned()))
    } else {
        Ok(())
    }
}

fn join(root: &str, relative: &str) -> String {
    format!("{}/{}", root.trim_end_matches('/'), relative)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::BTreeMap};

    #[derive(Default)]
    struct FakeWriter {
        files: BTreeMap<String, Vec<u8>>,
        writes: Vec<String>,
    }
    impl DerivedWriter for FakeWriter {
        fn exists(&self, path: &str) -> Result<bool, String> {
            Ok(self.files.contains_key(path))
        }
        fn create_new(&mut self, path: &str, bytes: &[u8]) -> Result<(), String> {
            if self.files.contains_key(path) {
                return Err("already exists".into());
            }
            self.files.insert(path.into(), bytes.into());
            self.writes.push(path.into());
            Ok(())
        }
    }

    fn item(name: &str, woa: Option<WoAContext>) -> ExportItem {
        ExportItem {
            provenance: Provenance {
                session_id: "session-1".into(),
                gallery_id: "gallery-1".into(),
                photo_id: "photo-1".into(),
                source_name: "original.raw".into(),
                recipe_revision: Some("r2".into()),
                exported_at_utc: "2026-11-18T12:00:00Z".into(),
            },
            destination: name.into(),
            derived_bytes: b"derived-preview".to_vec(),
            woa,
        }
    }
    fn complete_woa() -> Option<WoAContext> {
        Some(WoAContext {
            program: Some("W:O:A 2026".into()),
            event_context: Some("Main stage".into()),
        })
    }

    #[test]
    fn plan_is_reviewable_before_any_write_and_apply_writes_only_derived_data() {
        let originals = RefCell::new(BTreeMap::from([(
            "/source/original.raw".to_string(),
            b"raw bytes".to_vec(),
        )]));
        let before = originals.borrow().clone();
        let mut writer = FakeWriter::default();
        let plan = plan_export(
            "/exports/session-1",
            vec![item("photo-1/preview.ppm", complete_woa())],
            &writer,
        )
        .unwrap();
        assert_eq!(plan.items[0].disposition, ItemDisposition::Ready);
        assert!(writer.writes.is_empty(), "planning must not write files");
        let report = apply_plan(&plan, &mut writer).unwrap();
        assert_eq!(
            report.written,
            vec!["/exports/session-1/photo-1/preview.ppm"]
        );
        assert_eq!(report.provenance[0].session_id, "session-1");
        assert_eq!(*originals.borrow(), before);
        assert_eq!(
            writer.files["/exports/session-1/photo-1/preview.ppm"],
            b"derived-preview"
        );
    }

    #[test]
    fn collisions_are_reported_and_existing_destinations_are_preserved() {
        let path = "/exports/session-1/photo-1/preview.ppm";
        let mut writer = FakeWriter {
            files: BTreeMap::from([(path.into(), b"existing".to_vec())]),
            writes: vec![],
        };
        let plan = plan_export(
            "/exports/session-1",
            vec![item("photo-1/preview.ppm", complete_woa())],
            &writer,
        )
        .unwrap();
        assert_eq!(plan.items[0].disposition, ItemDisposition::Collision);
        let report = apply_plan(&plan, &mut writer).unwrap();
        assert_eq!(report.collisions, vec![path]);
        assert_eq!(writer.files[path], b"existing");
        assert!(writer.writes.is_empty());
    }

    #[test]
    fn missing_program_or_context_requires_review_without_any_quota() {
        let mut writer = FakeWriter::default();
        let incomplete = Some(WoAContext {
            program: None,
            event_context: Some("Arena".into()),
        });
        let plan = plan_export(
            "/exports/session-1",
            vec![item("one.ppm", incomplete), item("two.ppm", None)],
            &writer,
        )
        .unwrap();
        assert!(
            matches!(&plan.items[0].disposition, ItemDisposition::NeedsReview(reasons) if reasons.iter().any(|r| r.contains("program")))
        );
        assert!(matches!(
            &plan.items[1].disposition,
            ItemDisposition::NeedsReview(_)
        ));
        let report = apply_plan(&plan, &mut writer).unwrap();
        assert_eq!(report.skipped_for_review.len(), 2);
        assert!(report.written.is_empty());
        assert!(writer.writes.is_empty());
    }

    #[test]
    fn unsafe_and_duplicate_destinations_fail_before_writes() {
        let writer = FakeWriter::default();
        assert!(matches!(
            plan_export(
                "/exports",
                vec![item("../original.raw", complete_woa())],
                &writer
            ),
            Err(ExportError::InvalidRelativePath(_))
        ));
        assert_eq!(
            plan_export(
                "/exports",
                vec![
                    item("same.ppm", complete_woa()),
                    item("same.ppm", complete_woa())
                ],
                &writer
            ),
            Err(ExportError::DuplicateDestination("same.ppm".into()))
        );
        assert!(writer.writes.is_empty());
    }
}
