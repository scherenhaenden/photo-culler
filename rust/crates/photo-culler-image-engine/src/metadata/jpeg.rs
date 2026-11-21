//! Bounded reader for the EXIF subset used by the current metadata model.
//!
//! This deliberately parses only JPEG APP1/TIFF structures. It does not decode
//! RAW containers, TIFF image files, XMP, or maker notes.

use std::{
    collections::HashSet,
    fs::File,
    io::{self, Read},
    path::Path,
};

use super::{PhotoMetadata, Source, TagRecord, normalize};

const JPEG_HEADER_LIMIT: usize = 16 * 1024 * 1024;
const MAX_JPEG_SEGMENTS: usize = 4096;
const MAX_IFD_DEPTH: usize = 4;
const MAX_IFD_ENTRIES: usize = 4096;
const MAX_TAG_BYTES: usize = 1024 * 1024;
const EXIF_PREFIX: &[u8] = b"Exif\0\0";

/// Stable failure categories for malformed or oversized JPEG metadata.
#[derive(Debug)]
pub enum ExifReadError {
    Io(io::Error),
    InvalidJpeg,
    Truncated,
    MalformedExif,
    ResourceLimit,
}

impl std::fmt::Display for ExifReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(_) => f.write_str("could not read photo metadata"),
            Self::InvalidJpeg => f.write_str("input is not a JPEG image"),
            Self::Truncated => f.write_str("JPEG or EXIF metadata is truncated"),
            Self::MalformedExif => f.write_str("JPEG contains malformed EXIF metadata"),
            Self::ResourceLimit => f.write_str("JPEG metadata exceeds the parser limit"),
        }
    }
}

impl std::error::Error for ExifReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for ExifReadError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

/// Parse common camera fields from JPEG EXIF APP1 segments.
///
/// No EXIF segment is a valid empty result. Malformed EXIF fails closed instead
/// of returning partially trusted field values. The returned raw tag strings
/// are intended for [`normalize`], which preserves provenance and reports
/// invalid field values as stable warnings.
pub fn parse_jpeg_exif(bytes: &[u8]) -> Result<Vec<TagRecord>, ExifReadError> {
    if bytes.len() < 2 || bytes[..2] != [0xff, 0xd8] {
        return Err(ExifReadError::InvalidJpeg);
    }

    let mut records = Vec::new();
    let mut cursor = 2usize;
    let mut segments = 0usize;
    loop {
        if cursor >= bytes.len() {
            return Err(ExifReadError::Truncated);
        }
        if bytes[cursor] != 0xff {
            return Err(ExifReadError::MalformedExif);
        }
        while cursor < bytes.len() && bytes[cursor] == 0xff {
            cursor += 1;
        }
        let marker = *bytes.get(cursor).ok_or(ExifReadError::Truncated)?;
        cursor += 1;
        if marker == 0xd9 || marker == 0xda {
            break;
        }
        if marker == 0x00 {
            return Err(ExifReadError::MalformedExif);
        }
        if is_standalone_marker(marker) {
            continue;
        }

        segments += 1;
        if segments > MAX_JPEG_SEGMENTS {
            return Err(ExifReadError::ResourceLimit);
        }
        let length = read_be_u16(bytes, cursor).ok_or(ExifReadError::Truncated)? as usize;
        if length < 2 {
            return Err(ExifReadError::MalformedExif);
        }
        let payload_start = cursor.checked_add(2).ok_or(ExifReadError::ResourceLimit)?;
        let segment_end = cursor
            .checked_add(length)
            .ok_or(ExifReadError::ResourceLimit)?;
        if segment_end > bytes.len() {
            return Err(ExifReadError::Truncated);
        }
        if marker == 0xe1 {
            let payload = &bytes[payload_start..segment_end];
            if let Some(tiff) = payload.strip_prefix(EXIF_PREFIX) {
                let mut parser = TiffParser::new(tiff);
                records.extend(parser.parse()?);
                if records.len() > MAX_IFD_ENTRIES {
                    return Err(ExifReadError::ResourceLimit);
                }
            }
        }
        cursor = segment_end;
    }
    // The specific capture timestamp is a better source than the generic TIFF
    // modification timestamp. Stable sorting retains file order for duplicates.
    records.sort_by_key(|record| match record.name.as_str() {
        "DateTimeOriginal" => 1,
        _ => 0,
    });
    Ok(records)
}

/// Parse JPEG EXIF and normalize recognized values with field-level provenance.
pub fn read_jpeg_metadata(bytes: &[u8]) -> Result<PhotoMetadata, ExifReadError> {
    parse_jpeg_exif(bytes).map(|records| normalize(&records))
}

/// Read only a bounded JPEG header for EXIF; image pixel data is not loaded.
pub fn read_jpeg_file(path: impl AsRef<Path>) -> Result<PhotoMetadata, ExifReadError> {
    let file = File::open(path)?;
    let mut header = Vec::new();
    file.take(JPEG_HEADER_LIMIT as u64)
        .read_to_end(&mut header)?;
    let reached_limit = header.len() == JPEG_HEADER_LIMIT;
    match read_jpeg_metadata(&header) {
        Err(ExifReadError::Truncated) if reached_limit => Err(ExifReadError::ResourceLimit),
        result => result,
    }
}

fn is_standalone_marker(marker: u8) -> bool {
    matches!(marker, 0x01 | 0xd0..=0xd8)
}

fn read_be_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes([
        *bytes.get(offset)?,
        *bytes.get(offset.checked_add(1)?)?,
    ]))
}

#[derive(Clone, Copy)]
enum ByteOrder {
    Little,
    Big,
}

struct TiffParser<'a> {
    bytes: &'a [u8],
    order: ByteOrder,
    visited: HashSet<u32>,
    entries_seen: usize,
    output: Vec<TagRecord>,
}

impl<'a> TiffParser<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            order: ByteOrder::Big,
            visited: HashSet::new(),
            entries_seen: 0,
            output: Vec::new(),
        }
    }

    fn parse(&mut self) -> Result<Vec<TagRecord>, ExifReadError> {
        if self.bytes.len() < 8 {
            return Err(ExifReadError::Truncated);
        }
        self.order = match &self.bytes[..2] {
            b"II" => ByteOrder::Little,
            b"MM" => ByteOrder::Big,
            _ => return Err(ExifReadError::MalformedExif),
        };
        if self.u16(2)? != 42 {
            return Err(ExifReadError::MalformedExif);
        }
        let first_ifd = self.u32(4)?;
        self.parse_ifd(first_ifd, 0)?;
        Ok(std::mem::take(&mut self.output))
    }

    fn parse_ifd(&mut self, offset: u32, depth: usize) -> Result<(), ExifReadError> {
        if depth > MAX_IFD_DEPTH {
            return Err(ExifReadError::ResourceLimit);
        }
        if offset == 0 || !self.visited.insert(offset) {
            return Ok(());
        }
        let offset = usize::try_from(offset).map_err(|_| ExifReadError::ResourceLimit)?;
        let count = self.u16(offset)? as usize;
        if count > MAX_IFD_ENTRIES {
            return Err(ExifReadError::ResourceLimit);
        }
        self.entries_seen = self
            .entries_seen
            .checked_add(count)
            .ok_or(ExifReadError::ResourceLimit)?;
        if self.entries_seen > MAX_IFD_ENTRIES {
            return Err(ExifReadError::ResourceLimit);
        }
        let entries_start = offset.checked_add(2).ok_or(ExifReadError::ResourceLimit)?;
        let entries_bytes = count.checked_mul(12).ok_or(ExifReadError::ResourceLimit)?;
        let entries_end = entries_start
            .checked_add(entries_bytes)
            .ok_or(ExifReadError::ResourceLimit)?;
        self.slice(entries_start, entries_end - entries_start)?;
        let mut exif_ifd_offset = None;
        for index in 0..count {
            let entry = entries_start + index * 12;
            let tag = self.u16(entry)?;
            let kind = self.u16(entry + 2)?;
            let value_count = self.u32(entry + 4)?;
            if tag == 0x8769 && kind == 4 && value_count == 1 {
                exif_ifd_offset = Some(self.u32(entry + 8)?);
            }
            if let Some((name, formatted)) = self.tag_value(tag, kind, value_count, entry)? {
                self.output.push(TagRecord {
                    source: Source::JpegExif,
                    name: name.to_owned(),
                    value: formatted,
                });
            }
        }
        if let Some(offset) = exif_ifd_offset {
            self.parse_ifd(offset, depth + 1)?;
        }
        // Validate the next-IFD pointer even though thumbnail IFDs are not
        // traversed. A corrupt pointer must not turn into unchecked arithmetic.
        let _next_ifd = self.u32(entries_end)?;
        Ok(())
    }

    fn tag_value(
        &self,
        tag: u16,
        kind: u16,
        count: u32,
        entry: usize,
    ) -> Result<Option<(&'static str, String)>, ExifReadError> {
        let name = match tag {
            0x010f => "Make",
            0x0110 => "Model",
            0x0112 => "Orientation",
            0x0132 => "DateTime",
            0x829a => "ExposureTime",
            0x829d => "FNumber",
            0x8827 => "PhotographicSensitivity",
            0x9003 => "DateTimeOriginal",
            0x9004 => "CreateDate",
            0x9011 => "OffsetTimeOriginal",
            0x9012 => "OffsetTime",
            0x920a => "FocalLength",
            _ => return Ok(None),
        };
        let data = self.entry_data(kind, count, entry)?;
        let value = match tag {
            0x010f | 0x0110 | 0x0132 | 0x9003 | 0x9004 | 0x9011 | 0x9012 => {
                if kind != 2 {
                    return Ok(Some((name, String::new())));
                }
                ascii_value(data)
            }
            0x0112 => {
                if kind != 3 || count != 1 || data.len() < 2 {
                    String::new()
                } else {
                    self.read_u16_slice(data, 0)?.to_string()
                }
            }
            0x829a | 0x829d | 0x920a => {
                if kind != 5 || count == 0 || data.len() < 8 {
                    String::new()
                } else {
                    let numerator = self.read_u32_slice(data, 0)?;
                    let denominator = self.read_u32_slice(data, 4)?;
                    format!("{numerator}/{denominator}")
                }
            }
            0x8827 => match (kind, count) {
                (3, 1) if data.len() >= 2 => self.read_u16_slice(data, 0)?.to_string(),
                (4, 1) if data.len() >= 4 => self.read_u32_slice(data, 0)?.to_string(),
                _ => String::new(),
            },
            _ => unreachable!(),
        };
        Ok(Some((name, value)))
    }

    fn entry_data(&self, kind: u16, count: u32, entry: usize) -> Result<&'a [u8], ExifReadError> {
        let size = match kind {
            1 | 2 | 6 | 7 => 1usize,
            3 | 8 => 2,
            4 | 9 | 11 => 4,
            5 | 10 | 12 => 8,
            _ => return Ok(&[]),
        };
        let length = usize::try_from(count)
            .ok()
            .and_then(|count| count.checked_mul(size))
            .ok_or(ExifReadError::ResourceLimit)?;
        if length > MAX_TAG_BYTES {
            return Err(ExifReadError::ResourceLimit);
        }
        if length <= 4 {
            return self.slice(entry + 8, length);
        }
        let offset = self.u32(entry + 8)?;
        let offset = usize::try_from(offset).map_err(|_| ExifReadError::ResourceLimit)?;
        self.slice(offset, length)
    }

    fn slice(&self, offset: usize, length: usize) -> Result<&'a [u8], ExifReadError> {
        let end = offset
            .checked_add(length)
            .ok_or(ExifReadError::ResourceLimit)?;
        self.bytes.get(offset..end).ok_or(ExifReadError::Truncated)
    }

    fn u16(&self, offset: usize) -> Result<u16, ExifReadError> {
        let data = self.slice(offset, 2)?;
        Ok(self.read_u16_slice(data, 0)?)
    }

    fn u32(&self, offset: usize) -> Result<u32, ExifReadError> {
        let data = self.slice(offset, 4)?;
        Ok(self.read_u32_slice(data, 0)?)
    }

    fn read_u16_slice(&self, bytes: &[u8], offset: usize) -> Result<u16, ExifReadError> {
        let value = [
            *bytes.get(offset).ok_or(ExifReadError::Truncated)?,
            *bytes.get(offset + 1).ok_or(ExifReadError::Truncated)?,
        ];
        Ok(match self.order {
            ByteOrder::Little => u16::from_le_bytes(value),
            ByteOrder::Big => u16::from_be_bytes(value),
        })
    }

    fn read_u32_slice(&self, bytes: &[u8], offset: usize) -> Result<u32, ExifReadError> {
        let value = [
            *bytes.get(offset).ok_or(ExifReadError::Truncated)?,
            *bytes.get(offset + 1).ok_or(ExifReadError::Truncated)?,
            *bytes.get(offset + 2).ok_or(ExifReadError::Truncated)?,
            *bytes.get(offset + 3).ok_or(ExifReadError::Truncated)?,
        ];
        Ok(match self.order {
            ByteOrder::Little => u32::from_le_bytes(value),
            ByteOrder::Big => u32::from_be_bytes(value),
        })
    }
}

fn ascii_value(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}
