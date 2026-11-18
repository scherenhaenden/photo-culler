//! Deterministic metadata normalization independent of the container parser.
//!
//! JPEG/TIFF/RAW and sidecar readers adapt their decoded tags to [`TagRecord`].
//! This layer retains the winning source and original text for every value.

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    JpegExif,
    RawEmbedded,
    Sidecar,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagRecord {
    pub source: Source,
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Provenance {
    pub source: Source,
    pub tag: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field<T> {
    pub value: Option<T>,
    pub original: Option<String>,
    pub provenance: Option<Provenance>,
}

impl<T> Default for Field<T> {
    fn default() -> Self {
        Self {
            value: None,
            original: None,
            provenance: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaptureTime {
    /// Camera-local wall time exactly as supplied by the winning tag.
    pub local_original: String,
    /// `Some` only when an offset was supplied and validated.
    pub utc_unix_seconds: Option<i64>,
    pub offset_minutes: Option<i32>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PhotoMetadata {
    pub capture_time: Field<CaptureTime>,
    pub orientation: Field<u16>,
    pub camera_make: Field<String>,
    pub camera_model: Field<String>,
    pub exposure_seconds: Field<Rational>,
    pub aperture: Field<Rational>,
    pub focal_length_mm: Field<Rational>,
    pub iso: Field<u32>,
    pub warnings: Vec<ParseWarning>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rational {
    pub numerator: u32,
    pub denominator: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseWarning {
    pub source: Source,
    pub tag: String,
    pub code: &'static str,
}

/// Merge parser output deterministically. Sidecar overrides embedded values;
/// equal-priority duplicates use input order, so callers should sort tags.
pub fn normalize(records: &[TagRecord]) -> PhotoMetadata {
    let mut result = PhotoMetadata::default();
    for rec in records {
        let key = canonical_key(&rec.name);
        let provenance = Provenance {
            source: rec.source,
            tag: rec.name.clone(),
        };
        match key.as_str() {
            "datetimeoriginal" | "createdate" => {
                let offset = find_offset(records, rec.source);
                match parse_capture(&rec.value, offset.as_deref()) {
                    Some(value) => assign(&mut result.capture_time, value, &rec.value, &provenance),
                    None => result.warnings.push(warning(rec, "invalid_capture_time")),
                }
            }
            "orientation" => parse_u16(rec)
                .filter(|v| (1..=8).contains(v))
                .map(|v| assign(&mut result.orientation, v, &rec.value, &provenance))
                .unwrap_or_else(|| result.warnings.push(warning(rec, "invalid_orientation"))),
            "make" => assign_string(&mut result.camera_make, rec, provenance),
            "model" => assign_string(&mut result.camera_model, rec, provenance),
            "exposuretime" => parse_rational(&rec.value)
                .map(|v| assign(&mut result.exposure_seconds, v, &rec.value, &provenance))
                .unwrap_or_else(|| result.warnings.push(warning(rec, "invalid_rational"))),
            "fnumber" => parse_rational(&rec.value)
                .map(|v| assign(&mut result.aperture, v, &rec.value, &provenance))
                .unwrap_or_else(|| result.warnings.push(warning(rec, "invalid_rational"))),
            "focallength" => parse_rational(&rec.value)
                .map(|v| assign(&mut result.focal_length_mm, v, &rec.value, &provenance))
                .unwrap_or_else(|| result.warnings.push(warning(rec, "invalid_rational"))),
            "photographicsensitivity" | "isospeedratings" | "iso" => parse_u32(rec)
                .map(|v| assign(&mut result.iso, v, &rec.value, &provenance))
                .unwrap_or_else(|| result.warnings.push(warning(rec, "invalid_iso"))),
            _ => {}
        }
    }
    result
}

fn assign<T: Clone>(field: &mut Field<T>, value: T, raw: &str, provenance: &Provenance) {
    if field
        .provenance
        .as_ref()
        .map(|p| p.source <= provenance.source)
        .unwrap_or(true)
    {
        field.value = Some(value);
        field.original = Some(raw.to_owned());
        field.provenance = Some(provenance.clone());
    }
}
fn assign_string(field: &mut Field<String>, rec: &TagRecord, provenance: Provenance) {
    let clean = rec.value.trim();
    if !clean.is_empty() {
        assign(field, clean.to_owned(), &rec.value, &provenance);
    }
}
fn canonical_key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
fn parse_u16(rec: &TagRecord) -> Option<u16> {
    rec.value.trim().parse().ok()
}
fn parse_u32(rec: &TagRecord) -> Option<u32> {
    rec.value.trim().parse().ok().filter(|v| *v > 0)
}
fn parse_rational(s: &str) -> Option<Rational> {
    let s = s.trim();
    let (n, d) = s.split_once('/')?;
    let numerator = n.trim().parse().ok()?;
    let denominator: u32 = d.trim().parse().ok()?;
    (denominator != 0).then_some(Rational {
        numerator,
        denominator,
    })
}
fn find_offset(records: &[TagRecord], source: Source) -> Option<String> {
    records
        .iter()
        .find(|r| {
            r.source == source
                && matches!(
                    canonical_key(&r.name).as_str(),
                    "offsettimeoriginal" | "offsettime"
                )
        })
        .map(|r| r.value.clone())
}
fn parse_capture(raw: &str, offset: Option<&str>) -> Option<CaptureTime> {
    let (date, time) = raw.trim().split_once(' ')?;
    let mut date = date.split(':');
    let (y, m, d) = (
        date.next()?.parse::<i32>().ok()?,
        date.next()?.parse::<u32>().ok()?,
        date.next()?.parse::<u32>().ok()?,
    );
    if date.next().is_some() {
        return None;
    }
    let mut time = time.split(':');
    let (hh, mm, ss) = (
        time.next()?.parse::<u32>().ok()?,
        time.next()?.parse::<u32>().ok()?,
        time.next()?.parse::<u32>().ok()?,
    );
    if time.next().is_some()
        || !(1..=12).contains(&m)
        || d < 1
        || d > days_in_month(y, m)
        || hh > 23
        || mm > 59
        || ss > 60
    {
        return None;
    }
    let (offset_minutes, utc_unix_seconds) = match offset {
        Some(s) => {
            let mins = parse_offset(s)?;
            (
                Some(mins),
                Some(unix_seconds(y, m, d, hh, mm, ss)? - i64::from(mins) * 60),
            )
        }
        None => (None, None),
    };
    Some(CaptureTime {
        local_original: raw.to_owned(),
        utc_unix_seconds,
        offset_minutes,
    })
}
fn parse_offset(s: &str) -> Option<i32> {
    let s = s.trim();
    let sign = match s.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };
    let (h, m) = s[1..].split_once(':')?;
    let (h, m) = (h.parse::<i32>().ok()?, m.parse::<i32>().ok()?);
    if h > 14 || m > 59 || (h == 14 && m != 0) {
        return None;
    }
    Some(sign * (h * 60 + m))
}
fn days_in_month(y: i32, m: u32) -> u32 {
    match m {
        2 if y % 4 == 0 && (y % 100 != 0 || y % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}
fn unix_seconds(y: i32, m: u32, d: u32, hh: u32, mm: u32, ss: u32) -> Option<i64> {
    let y = y - i32::from(m <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = m as i32 + if m > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + d as i32 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = i64::from(era * 146097 + doe - 719468);
    days.checked_mul(86400)?
        .checked_add(i64::from(hh * 3600 + mm * 60 + ss))
}
fn warning(rec: &TagRecord, code: &'static str) -> ParseWarning {
    ParseWarning {
        source: rec.source,
        tag: rec.name.clone(),
        code,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tag(source: Source, name: &str, value: &str) -> TagRecord {
        TagRecord {
            source,
            name: name.into(),
            value: value.into(),
        }
    }

    #[test]
    fn jpeg_raw_and_sidecar_values_keep_field_provenance_and_sidecar_wins() {
        let records = vec![
            tag(Source::JpegExif, "Make", " Camera Co "),
            tag(Source::JpegExif, "Model", "A1"),
            tag(Source::RawEmbedded, "ISO", "800"),
            tag(Source::RawEmbedded, "ExposureTime", "1/250"),
            tag(Source::RawEmbedded, "FNumber", "28/10"),
            tag(Source::RawEmbedded, "FocalLength", "50/1"),
            tag(Source::RawEmbedded, "Orientation", "6"),
            tag(
                Source::RawEmbedded,
                "DateTimeOriginal",
                "2024:02:29 12:00:00",
            ),
            tag(Source::RawEmbedded, "OffsetTimeOriginal", "+02:00"),
            tag(Source::Sidecar, "Model", "A1-Retouched"),
        ];
        let m = normalize(&records);
        assert_eq!(m.camera_make.value.as_deref(), Some("Camera Co"));
        assert_eq!(m.camera_model.value.as_deref(), Some("A1-Retouched"));
        assert_eq!(
            m.camera_model.provenance.as_ref().unwrap().source,
            Source::Sidecar
        );
        assert_eq!(m.orientation.value, Some(6));
        assert_eq!(m.iso.value, Some(800));
        assert_eq!(
            m.exposure_seconds.value,
            Some(Rational {
                numerator: 1,
                denominator: 250
            })
        );
        assert_eq!(
            m.capture_time.value.as_ref().unwrap().offset_minutes,
            Some(120)
        );
        assert_eq!(
            m.capture_time.value.as_ref().unwrap().utc_unix_seconds,
            Some(1709200800)
        );
        assert_eq!(
            m.capture_time.original.as_deref(),
            Some("2024:02:29 12:00:00")
        );
    }

    #[test]
    fn timezone_missing_is_explicit_and_malformed_values_warn_without_fabrication() {
        let no_zone = normalize(&[tag(
            Source::JpegExif,
            "DateTimeOriginal",
            "2023:01:02 03:04:05",
        )]);
        let capture = no_zone.capture_time.value.unwrap();
        assert_eq!(capture.local_original, "2023:01:02 03:04:05");
        assert_eq!(capture.offset_minutes, None);
        assert_eq!(capture.utc_unix_seconds, None);

        let bad = normalize(&[
            tag(Source::JpegExif, "DateTimeOriginal", "not a date"),
            tag(Source::JpegExif, "Orientation", "9"),
            tag(Source::Sidecar, "ISO", "bad"),
            tag(Source::Sidecar, "ExposureTime", "1/0"),
        ]);
        assert!(bad.capture_time.value.is_none());
        assert!(bad.orientation.value.is_none());
        assert!(bad.iso.value.is_none());
        assert_eq!(bad.warnings.len(), 4);
    }

    #[test]
    fn deterministic_json_projection_has_stable_missing_values_and_originals() {
        let m = normalize(&[
            tag(Source::JpegExif, "Model", "Cam"),
            tag(Source::JpegExif, "DateTimeOriginal", "2023:01:02 03:04:05"),
        ]);
        let projection = format!(
            "{{\"model\":\"{}\",\"model_source\":\"{:?}\",\"iso\":null,\"capture_local\":\"{}\",\"capture_utc\":null}}",
            m.camera_model.value.as_deref().unwrap(),
            m.camera_model.provenance.as_ref().unwrap().source,
            m.capture_time.value.as_ref().unwrap().local_original
        );
        assert_eq!(
            projection,
            "{\"model\":\"Cam\",\"model_source\":\"JpegExif\",\"iso\":null,\"capture_local\":\"2023:01:02 03:04:05\",\"capture_utc\":null}"
        );
    }
}
