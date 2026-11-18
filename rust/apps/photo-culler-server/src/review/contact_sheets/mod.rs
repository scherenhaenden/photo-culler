//! Deterministic contact sheets and manifest filtering.
//!
//! This module only reads source images and writes new PNG derivatives below the
//! caller's output directory. Decoding is injected so the server can use its
//! preview/cache adapter and tests can use tiny synthetic images.

use std::{collections::HashSet, fs, io, path::{Path, PathBuf}};

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestEntry {
    pub path: PathBuf,
    pub label: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OmittedEntry {
    pub line: Option<usize>,
    pub path: Option<PathBuf>,
    pub reason: OmissionReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OmissionReason {
    EmptyPath,
    Duplicate,
    Filtered,
    Unavailable(String),
    Decode(String),
}

#[derive(Clone, Debug, Default)]
pub struct ManifestFilter {
    /// If populated, only paths in this set are retained.
    pub include: Option<HashSet<PathBuf>>,
    /// Paths in this set are always omitted.
    pub exclude: HashSet<PathBuf>,
}

impl ManifestFilter {
    pub fn apply(&self, entries: impl IntoIterator<Item = (Option<usize>, ManifestEntry)>) -> (Vec<(Option<usize>, ManifestEntry)>, Vec<OmittedEntry>) {
        let mut included = Vec::new();
        let mut omitted = Vec::new();
        for (line, entry) in entries {
            if entry.path.as_os_str().is_empty() {
                omitted.push(OmittedEntry { line, path: Some(entry.path), reason: OmissionReason::EmptyPath });
            } else if self.exclude.contains(&entry.path) || self.include.as_ref().is_some_and(|set| !set.contains(&entry.path)) {
                omitted.push(OmittedEntry { line, path: Some(entry.path), reason: OmissionReason::Filtered });
            } else {
                included.push((line, entry));
            }
        }
        (included, omitted)
    }
}

/// Parse a UTF-8 CSV with `path` and optional `label` headers. Paths retain the
/// CSV order. Relative paths are resolved against `base`; malformed rows are
/// reported rather than silently discarded.
pub fn parse_csv_manifest(csv: &str, base: &Path) -> Result<(Vec<(Option<usize>, ManifestEntry)>, Vec<OmittedEntry>), String> {
    let records = parse_csv_records(csv)?;
    let Some((header_line, header)) = records.first() else { return Ok((Vec::new(), Vec::new())); };
    let path_column = header.iter().position(|v| v.trim().eq_ignore_ascii_case("path"))
        .ok_or_else(|| "CSV manifest requires a path header".to_owned())?;
    let label_column = header.iter().position(|v| v.trim().eq_ignore_ascii_case("label"));
    let mut entries = Vec::new();
    let mut omitted = Vec::new();
    for (line, fields) in records.iter().skip(1) {
        let raw = fields.get(path_column).map(String::as_str).unwrap_or("").trim();
        if raw.is_empty() {
            omitted.push(OmittedEntry { line: Some(*line), path: None, reason: OmissionReason::EmptyPath });
            continue;
        }
        let path = PathBuf::from(raw);
        let path = if path.is_absolute() { path } else { base.join(path) };
        let label = label_column.and_then(|i| fields.get(i)).cloned().unwrap_or_default();
        entries.push((Some(*line), ManifestEntry { path, label }));
    }
    let _ = header_line;
    Ok((entries, omitted))
}

/// Build a manifest from explicit paths while preserving caller order.
pub fn paths_manifest(paths: impl IntoIterator<Item = PathBuf>) -> Vec<(Option<usize>, ManifestEntry)> {
    paths.into_iter().map(|path| (None, ManifestEntry { label: path.file_name().unwrap_or_default().to_string_lossy().into_owned(), path })).collect()
}

pub trait PreviewDecoder {
    fn decode(&self, source: &Path) -> Result<RgbaImage, String>;
}

#[derive(Clone, Debug)]
pub struct SheetOptions {
    pub columns: u32,
    pub rows: u32,
    pub cell_width: u32,
    pub cell_height: u32,
    pub label_height: u32,
    pub margin: u32,
}

impl Default for SheetOptions {
    fn default() -> Self { Self { columns: 4, rows: 3, cell_width: 240, cell_height: 160, label_height: 28, margin: 12 } }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetPage {
    pub path: PathBuf,
    pub width: u32,
    pub height: u32,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContactSheetReport {
    pub pages: Vec<SheetPage>,
    pub omitted: Vec<OmittedEntry>,
}

pub fn render_contact_sheets<D: PreviewDecoder>(
    entries: impl IntoIterator<Item = (Option<usize>, ManifestEntry)>,
    filter: &ManifestFilter,
    decoder: &D,
    output_dir: &Path,
    options: &SheetOptions,
) -> io::Result<ContactSheetReport> {
    if options.columns == 0 || options.rows == 0 || options.cell_width == 0 || options.cell_height == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "sheet dimensions and grid must be positive"));
    }
    fs::create_dir_all(output_dir)?;
    let (filtered, mut omitted) = filter.apply(entries);
    let mut decoded = Vec::new();
    let mut seen = HashSet::new();
    for (line, entry) in filtered {
        if !seen.insert(entry.path.clone()) {
            omitted.push(OmittedEntry { line, path: Some(entry.path), reason: OmissionReason::Duplicate });
            continue;
        }
        match decoder.decode(&entry.path) {
            Ok(image) => decoded.push((entry, image)),
            Err(error) => omitted.push(OmittedEntry { line, path: Some(entry.path), reason: OmissionReason::Decode(error) }),
        }
    }

    let per_page = (options.columns * options.rows) as usize;
    let mut report = ContactSheetReport { pages: Vec::new(), omitted };
    for (page_index, chunk) in decoded.chunks(per_page).enumerate() {
        let width = options.margin * 2 + options.columns * options.cell_width;
        let height = options.margin * 2 + options.rows * (options.cell_height + options.label_height);
        let mut canvas = RgbaImage::from_pixel(width, height, Rgba([32, 32, 32, 255]));
        for (slot, (entry, source)) in chunk.iter().enumerate() {
            let col = slot as u32 % options.columns;
            let row = slot as u32 / options.columns;
            let x = options.margin + col * options.cell_width;
            let y = options.margin + row * (options.cell_height + options.label_height);
            let thumb = DynamicImage::ImageRgba8(source.clone()).thumbnail(options.cell_width, options.cell_height).to_rgba8();
            let px = x + (options.cell_width - thumb.width()) / 2;
            let py = y + (options.cell_height - thumb.height()) / 2;
            image::imageops::overlay(&mut canvas, &thumb, px.into(), py.into());
            // A deterministic label band is part of the output. Font rendering is
            // injected by the UI layer; this module returns the exact label per tile.
            let band_y = y + options.cell_height;
            for by in band_y..(band_y + options.label_height) {
                for bx in x..(x + options.cell_width) { canvas.put_pixel(bx, by, Rgba([16, 16, 16, 255])); }
            }
            draw_label(&mut canvas, x + 4, band_y + 4, &entry.label, x + options.cell_width - 4);
        }
        let path = output_dir.join(format!("contact-sheet-{:04}.png", page_index + 1));
        canvas.save_with_format(&path, ImageFormat::Png).map_err(io::Error::other)?;
        report.pages.push(SheetPage { path, width, height, entries: chunk.iter().map(|(entry, _)| entry.clone()).collect() });
    }
    Ok(report)
}

fn draw_label(canvas: &mut RgbaImage, mut x: u32, y: u32, text: &str, right: u32) {
    for ch in text.chars().flat_map(char::to_uppercase) {
        let rows = glyph(ch);
        if x + 5 >= right { break; }
        for (dy, bits) in rows.iter().enumerate() {
            for dx in 0..5 {
                if bits & (1 << (4 - dx)) != 0 && x + dx < canvas.width() && y + (dy as u32) < canvas.height() {
                    canvas.put_pixel(x + dx, y + dy as u32, Rgba([240, 240, 240, 255]));
                }
            }
        }
        x += 6;
    }
}

fn glyph(ch: char) -> [u8; 7] {
    match ch {
        'A' => [14,17,17,31,17,17,17], 'B' => [30,17,17,30,17,17,30], 'C' => [14,17,16,16,16,17,14],
        'D' => [30,17,17,17,17,17,30], 'E' => [31,16,16,30,16,16,31], 'F' => [31,16,16,30,16,16,16],
        'G' => [14,17,16,23,17,17,15], 'H' => [17,17,17,31,17,17,17], 'I' => [14,4,4,4,4,4,14],
        'J' => [7,2,2,2,18,18,12], 'K' => [17,18,20,24,20,18,17], 'L' => [16,16,16,16,16,16,31],
        'M' => [17,27,21,21,17,17,17], 'N' => [17,25,21,19,17,17,17], 'O' => [14,17,17,17,17,17,14],
        'P' => [30,17,17,30,16,16,16], 'Q' => [14,17,17,17,21,18,13], 'R' => [30,17,17,30,20,18,17],
        'S' => [15,16,16,14,1,1,30], 'T' => [31,4,4,4,4,4,4], 'U' => [17,17,17,17,17,17,14],
        'V' => [17,17,17,17,17,10,4], 'W' => [17,17,17,21,21,21,10], 'X' => [17,17,10,4,10,17,17],
        'Y' => [17,17,10,4,4,4,4], 'Z' => [31,1,2,4,8,16,31],
        '0' => [14,17,19,21,25,17,14], '1' => [4,12,4,4,4,4,14], '2' => [14,17,1,2,4,8,31],
        '3' => [30,1,1,14,1,1,30], '4' => [2,6,10,18,31,2,2], '5' => [31,16,16,30,1,1,30],
        '6' => [14,16,16,30,17,17,14], '7' => [31,1,2,4,8,8,8], '8' => [14,17,17,14,17,17,14],
        '9' => [14,17,17,15,1,1,14], '-' => [0,0,0,31,0,0,0], '_' => [0,0,0,0,0,0,31],
        '.' => [0,0,0,0,0,12,12], ':' => [0,12,12,0,12,12,0], '/' => [1,2,2,4,8,8,16],
        ' ' => [0;7], _ => [31,17,1,2,4,0,4],
    }
}

fn parse_csv_records(csv: &str) -> Result<Vec<(usize, Vec<String>)>, String> {
    let mut records = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = csv.char_indices().peekable();
    let mut line = 1usize;
    let mut row_line = 1usize;
    while let Some((_, ch)) = chars.next() {
        if quoted {
            if ch == '"' {
                if matches!(chars.peek(), Some((_, '"'))) { chars.next(); field.push('"'); }
                else { quoted = false; }
            } else { if ch == '\n' { line += 1; } field.push(ch); }
        } else {
            match ch {
                '"' if field.is_empty() => quoted = true,
                ',' => { row.push(std::mem::take(&mut field)); },
                '\n' => { row.push(std::mem::take(&mut field)); records.push((row_line, std::mem::take(&mut row))); line += 1; row_line = line; },
                '\r' => {},
                _ => field.push(ch),
            }
        }
    }
    if quoted { return Err(format!("unterminated quoted CSV field at line {row_line}")); }
    if !field.is_empty() || !row.is_empty() { row.push(field); records.push((row_line, row)); }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, process::Command, sync::atomic::{AtomicUsize, Ordering}};

    static NEXT: AtomicUsize = AtomicUsize::new(0);
    struct Temp(PathBuf);
    impl Temp { fn new() -> Self { let path = std::env::temp_dir().join(format!("contact-sheet-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))); let _ = fs::remove_dir_all(&path); fs::create_dir_all(&path).unwrap(); Self(path) } }
    impl Drop for Temp { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.0); } }
    struct Fake(HashMap<PathBuf, RgbaImage>);
    impl PreviewDecoder for Fake { fn decode(&self, p: &Path) -> Result<RgbaImage, String> { self.0.get(p).cloned().ok_or_else(|| "missing fixture".into()) } }
    fn hash(path: &Path) -> String { let out = Command::new("shasum").arg("-a").arg("256").arg(path).output().unwrap(); String::from_utf8(out.stdout).unwrap().split_whitespace().next().unwrap().to_owned() }

    #[test]
    fn csv_sheet_and_path_list_preserve_order_dimensions_labels_and_sources() {
        let temp = Temp::new();
        let originals = [temp.0.join("one.raw"), temp.0.join("two.jpg"), temp.0.join("three.jpg")];
        for (i, path) in originals.iter().enumerate() { fs::write(path, format!("original-{i}")).unwrap(); }
        let before: Vec<_> = originals.iter().map(|p| hash(p)).collect();
        let csv = "path,label\none.raw,Concert A\ntwo.jpg,Close-up\nthree.jpg,Encore\nmissing.jpg,Unavailable\n";
        let (parsed, parse_omitted) = parse_csv_manifest(csv, &temp.0).unwrap();
        assert!(parse_omitted.is_empty());
        let mut images = HashMap::new();
        for (i, path) in originals.iter().enumerate() { images.insert(path.clone(), RgbaImage::from_pixel(8, 6, Rgba([i as u8, 20, 40, 255]))); }
        let options = SheetOptions { columns: 2, rows: 2, cell_width: 20, cell_height: 10, label_height: 4, margin: 3 };
        let report = render_contact_sheets(parsed, &ManifestFilter::default(), &Fake(images.clone()), &temp.0.join("csv-out"), &options).unwrap();
        assert_eq!(report.pages.len(), 1);
        assert_eq!((report.pages[0].width, report.pages[0].height), (46, 34));
        assert_eq!(report.pages[0].entries.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(), ["Concert A", "Close-up", "Encore"]);
        let rendered = image::open(&report.pages[0].path).unwrap().to_rgba8();
        assert!(rendered.pixels().any(|p| *p == Rgba([240, 240, 240, 255])), "labels are rendered into the sheet");
        assert!(matches!(report.omitted[0].reason, OmissionReason::Decode(_)));
        let listed = paths_manifest(originals.iter().cloned());
        let list_report = render_contact_sheets(listed, &ManifestFilter::default(), &Fake(images), &temp.0.join("list-out"), &options).unwrap();
        assert_eq!(list_report.pages[0].entries.iter().map(|e| &e.path).collect::<Vec<_>>(), originals.iter().collect::<Vec<_>>());
        assert_eq!(originals.iter().map(|p| hash(p)).collect::<Vec<_>>(), before);
    }

    #[test]
    fn filter_duplicates_and_bad_csv_are_reported() {
        let temp = Temp::new();
        let path = temp.0.join("a.jpg");
        let input = paths_manifest([path.clone(), path.clone()]);
        let filter = ManifestFilter { include: Some(HashSet::from([path.clone()])), exclude: HashSet::new() };
        let report = render_contact_sheets(input, &filter, &Fake(HashMap::new()), &temp.0.join("out"), &SheetOptions::default()).unwrap();
        assert_eq!(report.omitted.len(), 2);
        assert!(matches!(report.omitted[0].reason, OmissionReason::Decode(_)));
        assert_eq!(report.omitted[1].reason, OmissionReason::Duplicate);
        assert!(parse_csv_manifest("name\na.jpg\n", &temp.0).is_err());
        assert!(parse_csv_manifest("path,label\n\"broken", &temp.0).is_err());
    }
}
