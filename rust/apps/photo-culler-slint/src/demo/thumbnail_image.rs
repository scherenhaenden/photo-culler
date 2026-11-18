//! Bounded, MIME-checked JPEG decoding shared by desktop and WASM.

pub const MAX_JPEG_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_EDGE: u32 = 4096;
pub const MAX_PIXELS: u64 = 16_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dimensions {
    pub width: u16,
    pub height: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThumbnailError {
    InvalidMime,
    Empty,
    TooLarge { limit: usize, actual: usize },
    InvalidJpeg,
    DimensionsOutOfBounds { width: u32, height: u32 },
    DecodeFailed,
}

/// Inspect JPEG frame headers before allocation so oversized images are
/// rejected before the Slint decoder expands their pixels.
pub fn validate_jpeg(bytes: &[u8], mime: &str) -> Result<Dimensions, ThumbnailError> {
    if !mime
        .split(';')
        .next()
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("image/jpeg"))
    {
        return Err(ThumbnailError::InvalidMime);
    }
    if bytes.is_empty() {
        return Err(ThumbnailError::Empty);
    }
    if bytes.len() > MAX_JPEG_BYTES {
        return Err(ThumbnailError::TooLarge {
            limit: MAX_JPEG_BYTES,
            actual: bytes.len(),
        });
    }
    if !bytes.starts_with(&[0xff, 0xd8]) || !bytes.ends_with(&[0xff, 0xd9]) {
        return Err(ThumbnailError::InvalidJpeg);
    }
    let mut pos = 2usize;
    while pos < bytes.len().saturating_sub(2) {
        if bytes[pos] != 0xff {
            return Err(ThumbnailError::InvalidJpeg);
        }
        while pos < bytes.len() && bytes[pos] == 0xff {
            pos += 1;
        }
        let marker = *bytes.get(pos).ok_or(ThumbnailError::InvalidJpeg)?;
        pos += 1;
        if marker == 0xd9 || marker == 0xda {
            break;
        }
        if (0xd0..=0xd7).contains(&marker) || marker == 0x01 {
            continue;
        }
        let length = u16::from_be_bytes([
            *bytes.get(pos).ok_or(ThumbnailError::InvalidJpeg)?,
            *bytes.get(pos + 1).ok_or(ThumbnailError::InvalidJpeg)?,
        ]) as usize;
        if length < 2 || pos.checked_add(length).is_none_or(|end| end > bytes.len()) {
            return Err(ThumbnailError::InvalidJpeg);
        }
        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) {
            if length < 7 {
                return Err(ThumbnailError::InvalidJpeg);
            }
            let height = u16::from_be_bytes([bytes[pos + 3], bytes[pos + 4]]);
            let width = u16::from_be_bytes([bytes[pos + 5], bytes[pos + 6]]);
            if width == 0
                || height == 0
                || width as u32 > MAX_EDGE
                || height as u32 > MAX_EDGE
                || width as u64 * height as u64 > MAX_PIXELS
            {
                return Err(ThumbnailError::DimensionsOutOfBounds {
                    width: width as u32,
                    height: height as u32,
                });
            }
            return Ok(Dimensions { width, height });
        }
        pos += length;
    }
    Err(ThumbnailError::InvalidJpeg)
}

/// Convert validated response bytes into a Slint image. The Slint package
/// enables its `image-decoders` feature for native and uses the browser decoder
/// on WASM; no filesystem path is involved on either platform.
pub fn decode_jpeg(bytes: &[u8], mime: &str) -> Result<slint::Image, ThumbnailError> {
    let dimensions = validate_jpeg(bytes, mime)?;
    let decoded = image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg)
        .map_err(|_| ThumbnailError::DecodeFailed)?
        .to_rgba8();
    let (width, height) = decoded.dimensions();
    if width != dimensions.width as u32 || height != dimensions.height as u32 {
        return Err(ThumbnailError::DecodeFailed);
    }
    let mut pixels = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
    pixels.make_mut_bytes().copy_from_slice(decoded.as_raw());
    Ok(slint::Image::from_rgba8(pixels))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{codecs::jpeg::JpegEncoder, ColorType};
    // Minimal SOF0 frame header with 640x480 dimensions, followed by SOS/EOI.
    const JPEG_640X480: &[u8] = &[
        0xff, 0xd8, 0xff, 0xc0, 0x00, 0x11, 0x08, 0x01, 0xe0, 0x02, 0x80, 0x03, 0x01, 0x11, 0x00,
        0x02, 0x11, 0x00, 0x03, 0x11, 0x00, 0xff, 0xda, 0x00, 0x02, 0xff, 0xd9,
    ];
    #[test]
    fn valid_fixture_preserves_dimensions() {
        assert_eq!(
            validate_jpeg(JPEG_640X480, "image/jpeg"),
            Ok(Dimensions {
                width: 640,
                height: 480
            })
        );
    }
    #[test]
    fn decodes_jpeg_bytes_into_slint_image_with_dimensions_preserved() {
        let mut bytes = Vec::new();
        JpegEncoder::new(&mut bytes)
            .encode(&[255, 0, 0, 0, 255, 0], 2, 1, ColorType::Rgb8.into())
            .unwrap();
        let image = decode_jpeg(&bytes, "image/jpeg").unwrap();
        assert_eq!(image.size().width, 2);
        assert_eq!(image.size().height, 1);
    }
    #[test]
    fn rejects_invalid_mime_and_truncated_data() {
        assert_eq!(
            validate_jpeg(JPEG_640X480, "text/plain"),
            Err(ThumbnailError::InvalidMime)
        );
        assert_eq!(
            validate_jpeg(&JPEG_640X480[..JPEG_640X480.len() - 1], "image/jpeg"),
            Err(ThumbnailError::InvalidJpeg)
        );
        assert_eq!(
            validate_jpeg(&[0xff, 0xd8, 0xff, 0xd9], "image/jpeg"),
            Err(ThumbnailError::InvalidJpeg)
        );
    }
    #[test]
    fn a_truncated_image_is_not_available() {
        assert_eq!(decode_jpeg(&JPEG_640X480[..JPEG_640X480.len()-1], "image/jpeg"), Err(ThumbnailError::InvalidJpeg));
    }
    #[test]
    fn rejects_dimensions_before_decode() {
        let mut huge = JPEG_640X480.to_vec();
        huge[7] = 0x20;
        huge[8] = 0x00; // 8192px height
        assert!(matches!(
            validate_jpeg(&huge, "image/jpeg"),
            Err(ThumbnailError::DimensionsOutOfBounds { .. })
        ));
    }
    #[test]
    fn rejects_response_over_limit() {
        let bytes = vec![0; MAX_JPEG_BYTES + 1];
        assert!(matches!(
            validate_jpeg(&bytes, "image/jpeg"),
            Err(ThumbnailError::TooLarge { .. })
        ));
    }
}
