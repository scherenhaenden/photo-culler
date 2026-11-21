//! Deterministic JPEG burst preselection and legacy-compatible CSV output.
//!
//! Image decoding and metadata extraction are intentionally supplied by the
//! caller. Selection is non-destructive and operates only on descriptors.

use std::path::{Path, PathBuf};

const RAW_SUFFIXES: [&str; 8] = [
    ".NEF", ".nef", ".ARW", ".arw", ".CR3", ".cr3", ".DNG", ".dng",
];
const CSV_HEADER: &str =
    "jpeg_path,raw_path,group,selected,representative_jpeg,quality,similarity\r\n";

#[derive(Clone, Debug, PartialEq)]
pub struct FrameDescriptor {
    pub jpeg_path: PathBuf,
    /// Capture time in Unix milliseconds, normalized by the metadata adapter.
    pub captured_at_unix_millis: i64,
    pub sequence_number: Option<u64>,
    /// 256-bit perceptual hash, least significant bit first in each word.
    pub dhash: [u64; 4],
    /// RGB means for a row-major 4x4 grid.
    pub colour_grid: [f32; 48],
    /// Concatenated normalized 12-bin RGB histograms.
    pub histogram: [f32; 36],
    pub quality: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreselectorConfig {
    pub max_gap_millis: u64,
    pub max_sequence_gap: u64,
    pub duplicate_similarity: f64,
}

impl Default for PreselectorConfig {
    fn default() -> Self {
        Self {
            max_gap_millis: 4_000,
            max_sequence_gap: 14,
            duplicate_similarity: 0.88,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Preselection {
    pub frame: FrameDescriptor,
    pub group_id: String,
    pub selected: bool,
    pub representative_jpeg: PathBuf,
    pub similarity_to_representative: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PreselectionError {
    InvalidThresholds,
    InvalidDescriptor { path: PathBuf, reason: &'static str },
}

/// Select one technically strongest frame from each adjacent burst.
pub fn select(
    mut frames: Vec<FrameDescriptor>,
    config: PreselectorConfig,
) -> Result<Vec<Preselection>, PreselectionError> {
    if config.max_gap_millis == 0
        || config.max_sequence_gap == 0
        || !config.duplicate_similarity.is_finite()
        || !(0.0 < config.duplicate_similarity && config.duplicate_similarity <= 1.0)
    {
        return Err(PreselectionError::InvalidThresholds);
    }
    for frame in &frames {
        if !frame.quality.is_finite()
            || frame
                .colour_grid
                .iter()
                .chain(frame.histogram.iter())
                .any(|v| !v.is_finite())
        {
            return Err(PreselectionError::InvalidDescriptor {
                path: frame.jpeg_path.clone(),
                reason: "non-finite score or colour value",
            });
        }
    }
    frames.sort_by_key(sort_key);
    if frames.is_empty() {
        return Ok(Vec::new());
    }

    let mut clusters: Vec<Vec<FrameDescriptor>> = vec![vec![frames.remove(0)]];
    for frame in frames {
        let previous = clusters
            .last()
            .expect("initialized with one frame")
            .last()
            .expect("cluster is nonempty");
        if nearby(previous, &frame, config)
            && similarity(previous, &frame) >= config.duplicate_similarity
        {
            clusters
                .last_mut()
                .expect("initialized with one cluster")
                .push(frame);
        } else {
            clusters.push(vec![frame]);
        }
    }

    let mut results = Vec::new();
    for (index, cluster) in clusters.into_iter().enumerate() {
        let representative_index = cluster
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| {
                left.quality.total_cmp(&right.quality).then_with(|| {
                    path_name(&left.jpeg_path)
                        .to_lowercase()
                        .cmp(&path_name(&right.jpeg_path).to_lowercase())
                })
            })
            .map(|(i, _)| i)
            .expect("cluster is nonempty");
        let representative = cluster[representative_index].jpeg_path.clone();
        let representative_frame = cluster[representative_index].clone();
        let group_id = format!("burst-{:04}", index + 1);
        for (frame_index, frame) in cluster.into_iter().enumerate() {
            let score = if frame_index == representative_index {
                1.0
            } else {
                similarity(&frame, &representative_frame)
            };
            results.push(Preselection {
                selected: frame_index == representative_index,
                similarity_to_representative: score,
                representative_jpeg: representative.clone(),
                frame,
                group_id: group_id.clone(),
            });
        }
    }
    Ok(results)
}

/// Similarity score matching the legacy 0.55/0.30/0.15 metric blend.
pub fn similarity(left: &FrameDescriptor, right: &FrameDescriptor) -> f64 {
    let differing = (0..4)
        .map(|i| (left.dhash[i] ^ right.dhash[i]).count_ones() as f64)
        .sum::<f64>();
    let hash = 1.0 - differing / 256.0;
    let grid_delta = left
        .colour_grid
        .iter()
        .zip(right.colour_grid.iter())
        .map(|(a, b)| (a - b).abs() as f64)
        .sum::<f64>()
        / 48.0;
    let grid = 1.0 - (grid_delta / 0.5).min(1.0);
    let histogram_delta = left
        .histogram
        .iter()
        .zip(right.histogram.iter())
        .map(|(a, b)| (a - b).abs() as f64)
        .sum::<f64>()
        / 36.0;
    let histogram = 1.0 - (histogram_delta * 12.0).min(1.0);
    round6(0.55 * hash + 0.30 * grid + 0.15 * histogram)
}

/// Resolve same-stem RAW paths in the legacy suffix priority order.
pub fn matching_raw_path(raw_paths: &[PathBuf], jpeg_path: &Path) -> Option<PathBuf> {
    let stem = jpeg_path.file_stem()?.to_str()?;
    RAW_SUFFIXES.iter().find_map(|suffix| {
        let filename = format!("{stem}{suffix}");
        raw_paths
            .iter()
            .find(|path| path.file_name().and_then(|name| name.to_str()) == Some(filename.as_str()))
            .cloned()
    })
}

/// Render the CSV manifest with Python `csv.DictWriter` compatible columns and CRLF.
pub fn write_csv(rows: &[Preselection], raw_paths: &[PathBuf]) -> String {
    let mut output = String::from(CSV_HEADER);
    for row in rows {
        let raw = matching_raw_path(raw_paths, &row.frame.jpeg_path)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();
        let fields = [
            row.frame.jpeg_path.to_string_lossy().into_owned(),
            raw,
            row.group_id.clone(),
            if row.selected {
                "yes".to_owned()
            } else {
                "no".to_owned()
            },
            row.representative_jpeg.to_string_lossy().into_owned(),
            format!("{:.4}", row.frame.quality),
            format!("{:.4}", row.similarity_to_representative),
        ];
        for (index, field) in fields.iter().enumerate() {
            if index > 0 {
                output.push(',');
            }
            csv_field(&mut output, field);
        }
        output.push_str("\r\n");
    }
    output
}

fn csv_field(output: &mut String, value: &str) {
    if value.contains([',', '"', '\r', '\n']) {
        output.push('"');
        for ch in value.chars() {
            if ch == '"' {
                output.push('"');
            }
            output.push(ch);
        }
        output.push('"');
    } else {
        output.push_str(value);
    }
}

fn nearby(left: &FrameDescriptor, right: &FrameDescriptor, config: PreselectorConfig) -> bool {
    let gap = left
        .captured_at_unix_millis
        .abs_diff(right.captured_at_unix_millis);
    if gap <= config.max_gap_millis {
        return true;
    }
    match (left.sequence_number, right.sequence_number) {
        (Some(a), Some(b)) => a.abs_diff(b) <= config.max_sequence_gap && gap <= 30_000,
        _ => false,
    }
}

fn sort_key(frame: &FrameDescriptor) -> (i64, u64, String) {
    (
        frame.captured_at_unix_millis,
        frame.sequence_number.unwrap_or(0),
        path_name(&frame.jpeg_path).to_lowercase(),
    )
}
fn path_name(path: &Path) -> &str {
    path.file_name().and_then(|n| n.to_str()).unwrap_or("")
}
fn round6(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(
        path: &str,
        at: i64,
        sequence: Option<u64>,
        quality: f64,
        hash: u64,
    ) -> FrameDescriptor {
        FrameDescriptor {
            jpeg_path: path.into(),
            captured_at_unix_millis: at,
            sequence_number: sequence,
            dhash: [hash; 4],
            colour_grid: [0.25; 48],
            histogram: [1.0 / 36.0; 36],
            quality,
        }
    }

    #[test]
    fn acceptance_csv_matches_reviewed_legacy_fixture_byte_for_byte() {
        let mut first = frame("/photos/DSC, 0001.jpg", 1_700_000_000_000, Some(1), 0.7, 0);
        let second = frame("/photos/DSC, 0002.jpg", 1_700_000_001_000, Some(2), 0.8, 1);
        first.dhash = [0, 0, 0, 0];
        let solo = frame(
            "/photos/solo.jpg",
            1_700_000_100_000,
            Some(10),
            0.65,
            u64::MAX,
        );
        let rows = select(
            vec![solo, second, first],
            PreselectorConfig {
                max_gap_millis: 4_000,
                max_sequence_gap: 14,
                duplicate_similarity: 0.88,
            },
        )
        .unwrap();
        let raws = vec![
            PathBuf::from("/raw/DSC, 0001.NEF"),
            PathBuf::from("/raw/DSC, 0002.ARW"),
        ];
        let actual = write_csv(&rows, &raws);
        let expected = include_bytes!("fixtures/preselection.csv");
        assert_eq!(actual.as_bytes(), expected);
    }

    #[test]
    fn same_quality_tie_uses_legacy_larger_casefolded_name() {
        let a = frame("/photos/DSC_0001.jpg", 0, Some(1), 0.8, 0);
        let b = frame("/photos/DSC_0002.jpg", 1, Some(2), 0.8, 0);
        let rows = select(vec![a, b], PreselectorConfig::default()).unwrap();
        assert!(rows[1].selected);
    }

    #[test]
    fn raw_matching_obeys_legacy_suffix_order_and_missing_is_empty() {
        let raws = vec![PathBuf::from("/raw/A.ARW"), PathBuf::from("/raw/A.NEF")];
        assert_eq!(
            matching_raw_path(&raws, Path::new("/photos/A.jpg")),
            Some(PathBuf::from("/raw/A.NEF"))
        );
        assert_eq!(matching_raw_path(&raws, Path::new("/photos/B.jpg")), None);
    }
}
