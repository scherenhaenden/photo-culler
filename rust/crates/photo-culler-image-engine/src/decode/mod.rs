//! Bounded raster decoding and RAW preview selection.
//!
//! This module deliberately does not claim to demosaic proprietary RAW data.
//! The current workspace enables JPEG, PNG, TIFF, BMP, GIF, and WebP codecs. A
//! RAW adapter should extract an embedded preview first and may then provide a
//! paired raster representation as fallback. Camera-specific RAW decoding can
//! be added behind `RawPreviewExtractor` after coverage and distribution are
//! verified.

use std::io::Cursor;

use image::{DynamicImage, ImageDecoder, ImageReader, Limits, metadata::Orientation};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DecodeLimits {
    pub max_width: u32,
    pub max_height: u32,
    pub max_alloc_bytes: u64,
}

impl Default for DecodeLimits {
    fn default() -> Self {
        Self {
            max_width: 120_000,
            max_height: 120_000,
            max_alloc_bytes: 512 * 1024 * 1024,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    UnsupportedFormat,
    InvalidData(String),
    DimensionsExceeded { width: u32, height: u32 },
    AllocationLimit,
    RawPreviewUnavailable,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepresentationSource {
    Raster,
    EmbeddedRawPreview,
    PairedRasterFallback,
}

#[derive(Debug)]
pub struct DecodedImage {
    pub image: DynamicImage,
    pub source: RepresentationSource,
    pub orientation: Orientation,
}

/// Decode a supported raster format, enforcing dimensions before allocation
/// and applying the container's EXIF orientation to the returned pixels.
pub fn decode_raster(bytes: &[u8], limits: DecodeLimits) -> Result<DecodedImage, DecodeError> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| DecodeError::InvalidData(error.to_string()))?;
    let mut decoder_limits = Limits::default();
    decoder_limits.max_image_width = Some(limits.max_width);
    decoder_limits.max_image_height = Some(limits.max_height);
    decoder_limits.max_alloc = Some(limits.max_alloc_bytes);
    reader.limits(decoder_limits.clone());
    let mut decoder = reader.into_decoder().map_err(map_error)?;
    let (width, height) = decoder.dimensions();
    if width > limits.max_width || height > limits.max_height {
        return Err(DecodeError::DimensionsExceeded { width, height });
    }
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    decoder_limits
        .reserve(decoder.total_bytes())
        .map_err(map_error)?;
    decoder.set_limits(decoder_limits).map_err(map_error)?;
    let mut image = DynamicImage::from_decoder(decoder).map_err(map_error)?;
    image.apply_orientation(orientation);
    Ok(DecodedImage {
        image,
        source: RepresentationSource::Raster,
        orientation,
    })
}

pub trait RawPreviewExtractor {
    /// Return the embedded preview bytes, if the RAW container has one.
    fn embedded_preview(&self, raw: &[u8]) -> Result<Option<Vec<u8>>, DecodeError>;
}

pub trait PairedRasterFallback {
    /// Return the already-associated JPEG/other raster bytes for this RAW.
    fn paired_raster(&self) -> Result<Option<Vec<u8>>, DecodeError>;
}

/// Prefer an embedded RAW preview. If it is absent or corrupt, try the paired
/// raster supplied by the catalog. No paths or pairing rules are inferred here.
pub fn decode_raw_preview<E, F>(
    raw: &[u8],
    extractor: &E,
    fallback: &F,
    limits: DecodeLimits,
) -> Result<DecodedImage, DecodeError>
where
    E: RawPreviewExtractor,
    F: PairedRasterFallback,
{
    if let Ok(Some(bytes)) = extractor.embedded_preview(raw) {
        if let Ok(mut decoded) = decode_raster(&bytes, limits) {
            decoded.source = RepresentationSource::EmbeddedRawPreview;
            return Ok(decoded);
        }
    }
    if let Some(bytes) = fallback.paired_raster()? {
        let mut decoded = decode_raster(&bytes, limits)?;
        decoded.source = RepresentationSource::PairedRasterFallback;
        return Ok(decoded);
    }
    Err(DecodeError::RawPreviewUnavailable)
}

fn map_error(error: image::ImageError) -> DecodeError {
    match error {
        image::ImageError::Unsupported(_) => DecodeError::UnsupportedFormat,
        image::ImageError::Limits(limit) => match limit.kind() {
            image::error::LimitErrorKind::DimensionError { .. } => {
                DecodeError::DimensionsExceeded {
                    width: 0,
                    height: 0,
                }
            }
            _ => DecodeError::AllocationLimit,
        },
        other => DecodeError::InvalidData(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, ImageFormat, Rgb, codecs::jpeg::JpegEncoder};

    fn limits() -> DecodeLimits {
        DecodeLimits {
            max_width: 100,
            max_height: 100,
            max_alloc_bytes: 1_000_000,
        }
    }

    fn jpeg_with_orientation(tag: u16) -> Vec<u8> {
        let image = ImageBuffer::from_pixel(3, 2, Rgb([40u8, 80, 120]));
        let mut jpeg = Vec::new();
        JpegEncoder::new(&mut jpeg).encode_image(&image).unwrap();
        let payload = [
            b'E', b'x', b'i', b'f', 0, 0, b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 1, 0x01, 0x12, 0, 3, 0,
            0, 0, 1, 0, tag as u8, 0, 0, 0, 0, 0, 0,
        ];
        let mut exif = vec![0xff, 0xe1, 0, (payload.len() + 2) as u8];
        exif.extend_from_slice(&payload);
        jpeg.splice(2..2, exif);
        jpeg
    }

    #[test]
    fn decodes_jpeg_png_and_tiff_rasters() {
        for format in [ImageFormat::Jpeg, ImageFormat::Png, ImageFormat::Tiff] {
            let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(4, 3, Rgb([1, 2, 3])));
            let mut bytes = Cursor::new(Vec::new());
            image.write_to(&mut bytes, format).unwrap();
            let decoded = decode_raster(bytes.get_ref(), limits()).unwrap();
            assert_eq!((decoded.image.width(), decoded.image.height()), (4, 3));
        }
    }

    #[test]
    fn applies_rotated_and_reflected_exif_orientation() {
        let rotated = decode_raster(&jpeg_with_orientation(6), limits()).unwrap();
        assert_eq!((rotated.image.width(), rotated.image.height()), (2, 3));
        let reflected = decode_raster(&jpeg_with_orientation(2), limits()).unwrap();
        assert_eq!((reflected.image.width(), reflected.image.height()), (3, 2));
    }

    #[test]
    fn rejects_truncated_data_and_excessive_dimensions() {
        assert!(matches!(
            decode_raster(&[0xff, 0xd8, 0xff], limits()),
            Err(DecodeError::InvalidData(_))
        ));
        let image = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(4, 3, Rgb([1, 2, 3])));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::Png).unwrap();
        assert!(matches!(
            decode_raster(
                bytes.get_ref(),
                DecodeLimits {
                    max_width: 3,
                    ..limits()
                }
            ),
            Err(DecodeError::DimensionsExceeded {
                width: 4,
                height: 3
            })
        ));
    }

    struct Preview(Option<Vec<u8>>);
    impl RawPreviewExtractor for Preview {
        fn embedded_preview(&self, _: &[u8]) -> Result<Option<Vec<u8>>, DecodeError> {
            Ok(self.0.clone())
        }
    }
    struct Fallback(Option<Vec<u8>>);
    impl PairedRasterFallback for Fallback {
        fn paired_raster(&self) -> Result<Option<Vec<u8>>, DecodeError> {
            Ok(self.0.clone())
        }
    }

    #[test]
    fn raw_prefers_embedded_preview_then_uses_paired_raster() {
        let mut preview = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(ImageBuffer::from_pixel(2, 2, Rgb([8, 9, 10])))
            .write_to(&mut preview, ImageFormat::Png)
            .unwrap();
        let mut fallback = Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(ImageBuffer::from_pixel(3, 2, Rgb([8, 9, 10])))
            .write_to(&mut fallback, ImageFormat::Png)
            .unwrap();
        let embedded = decode_raw_preview(
            b"raw",
            &Preview(Some(preview.into_inner())),
            &Fallback(None),
            limits(),
        )
        .unwrap();
        assert_eq!(embedded.source, RepresentationSource::EmbeddedRawPreview);
        let paired = decode_raw_preview(
            b"raw",
            &Preview(None),
            &Fallback(Some(fallback.into_inner())),
            limits(),
        )
        .unwrap();
        assert_eq!(paired.source, RepresentationSource::PairedRasterFallback);
    }
}
