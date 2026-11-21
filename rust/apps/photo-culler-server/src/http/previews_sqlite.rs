//! SQLite-backed preview generation from catalog-resolved source files.

use std::{
    collections::{HashMap, VecDeque},
    fs::File,
    io::Read,
    path::Path,
    sync::{Arc, Mutex},
};

use image::{DynamicImage, ImageDecoder, ImageReader, codecs::jpeg::JpegEncoder};
use sha2::{Digest, Sha256};
use tokio::runtime::Handle;
use tokio::sync::Semaphore;

use crate::{
    catalog::sqlite::Catalog,
    http::previews::{PreviewApi, PreviewApiError, PreviewApiFuture, PreviewAsset},
    media::previews::{
        GeneratedPreview, JPEG_QUALITY, MAX_DECODED_PIXELS, MAX_PREVIEW_DIMENSION, Orientation,
        PreviewCacheKey, PreviewFormat, PreviewPreset, RgbImage, orient_and_bound,
    },
};

const MAX_SOURCE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_SOURCE_DIMENSION: u32 = 120_000;
const MAX_SOURCE_ALLOCATION: u64 = 128 * 1024 * 1024;
const MAX_PREVIEW_BYTES: usize = 16 * 1024 * 1024;
const MAX_CACHE_BYTES: usize = 256 * 1024 * 1024;

#[derive(Clone)]
pub struct SqlitePreviewApi {
    catalog: Catalog,
    cache: Arc<PreviewCache>,
    runtime: Handle,
    permits: Arc<Semaphore>,
}

impl SqlitePreviewApi {
    pub fn new(catalog: Catalog, runtime: Handle) -> Self {
        Self {
            catalog,
            cache: Arc::new(PreviewCache::default()),
            runtime,
            permits: Arc::new(Semaphore::new(2)),
        }
    }
}

impl PreviewApi for SqlitePreviewApi {
    fn preview<'a>(
        &'a self,
        photo_id: &'a str,
        preset: PreviewPreset,
    ) -> PreviewApiFuture<'a, Result<PreviewAsset, PreviewApiError>> {
        Box::pin(async move {
            let path = self
                .catalog
                .display_path(photo_id)
                .await
                .map_err(|_| PreviewApiError::Unavailable)?
                .ok_or(PreviewApiError::NotFound)?;
            let cache = self.cache.clone();
            let task_path = path.clone();
            let photo_id = photo_id.to_owned();
            let runtime = self.runtime.clone();
            let permit = self
                .permits
                .clone()
                .try_acquire_owned()
                .map_err(|_| PreviewApiError::Busy)?;
            runtime
                .spawn_blocking(move || {
                    let _permit = permit;
                    render_cached(&cache, &photo_id, &task_path, preset)
                })
                .await
                .map_err(|_| PreviewApiError::Unavailable)?
        })
    }
}

#[derive(Default)]
struct PreviewCache {
    entries: Mutex<CacheState>,
}

#[derive(Default)]
struct CacheState {
    bytes: usize,
    order: VecDeque<PreviewCacheKey>,
    entries: HashMap<PreviewCacheKey, Arc<GeneratedPreview>>,
}

impl PreviewCache {
    fn get(&self, key: &PreviewCacheKey) -> Result<Option<Arc<GeneratedPreview>>, PreviewApiError> {
        self.entries
            .lock()
            .map(|state| state.entries.get(key).cloned())
            .map_err(|_| PreviewApiError::Unavailable)
    }

    fn put(
        &self,
        key: PreviewCacheKey,
        preview: Arc<GeneratedPreview>,
    ) -> Result<(), PreviewApiError> {
        if preview.bytes.len() > MAX_CACHE_BYTES {
            return Ok(());
        }
        let mut state = self
            .entries
            .lock()
            .map_err(|_| PreviewApiError::Unavailable)?;
        if let Some(previous) = state.entries.remove(&key) {
            state.bytes = state.bytes.saturating_sub(previous.bytes.len());
            state.order.retain(|existing| existing != &key);
        }
        state.bytes = state.bytes.saturating_add(preview.bytes.len());
        state.order.push_back(key.clone());
        state.entries.insert(key, preview);
        while state.bytes > MAX_CACHE_BYTES {
            let Some(oldest) = state.order.pop_front() else {
                state.bytes = 0;
                state.entries.clear();
                break;
            };
            if let Some(removed) = state.entries.remove(&oldest) {
                state.bytes = state.bytes.saturating_sub(removed.bytes.len());
            }
        }
        Ok(())
    }
}

fn render_cached(
    cache: &PreviewCache,
    photo_id: &str,
    path: &Path,
    preset: PreviewPreset,
) -> Result<PreviewAsset, PreviewApiError> {
    let (bytes, hash) = read_and_hash(path)?;
    let hash_hex = hex(&hash);
    let key = PreviewCacheKey {
        photo_identity: photo_id.to_owned(),
        source_revision: hash_hex.clone(),
        original_hash: hash_hex.clone(),
        render_version: crate::media::previews::PREVIEW_RENDER_VERSION.to_owned(),
        preset,
        max_dimension: preset.max_dimension().min(MAX_PREVIEW_DIMENSION),
        format: PreviewFormat::Jpeg,
        quality: JPEG_QUALITY,
        orientation: Orientation::Normal,
    };
    if let Some(preview) = cache.get(&key)? {
        return Ok(asset(preview));
    }
    let preview = Arc::new(render(bytes, hash_hex, preset)?);
    cache.put(key, preview.clone())?;
    Ok(asset(preview))
}

fn read_and_hash(path: &Path) -> Result<(Vec<u8>, [u8; 32]), PreviewApiError> {
    let canonical_path = path.canonicalize().map_err(|_| PreviewApiError::NotFound)?;
    let metadata = canonical_path
        .metadata()
        .map_err(|_| PreviewApiError::NotFound)?;
    if !metadata.is_file() {
        return Err(PreviewApiError::NotFound);
    }
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err(PreviewApiError::TooLarge);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(canonical_path)
        .map_err(|_| PreviewApiError::Unavailable)?
        .take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| PreviewApiError::Unavailable)?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(PreviewApiError::TooLarge);
    }
    let hash: [u8; 32] = Sha256::digest(&bytes).into();
    Ok((bytes, hash))
}

fn render(
    bytes: Vec<u8>,
    source_hash: String,
    preset: PreviewPreset,
) -> Result<GeneratedPreview, PreviewApiError> {
    let reader = ImageReader::new(std::io::Cursor::new(bytes.as_slice()))
        .with_guessed_format()
        .map_err(|_| PreviewApiError::UnsupportedSource)?;
    let (width, height) = reader
        .into_dimensions()
        .map_err(|_| PreviewApiError::UnsupportedSource)?;
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or(PreviewApiError::TooLarge)?;
    if width == 0 || height == 0 || pixels > MAX_DECODED_PIXELS {
        return Err(PreviewApiError::TooLarge);
    }

    let mut decoder = ImageReader::new(std::io::Cursor::new(bytes.as_slice()))
        .with_guessed_format()
        .map_err(|_| PreviewApiError::UnsupportedSource)?
        .into_decoder()
        .map_err(|_| PreviewApiError::UnsupportedSource)?;
    let (decoded_width, decoded_height) = decoder.dimensions();
    if decoded_width > MAX_SOURCE_DIMENSION || decoded_height > MAX_SOURCE_DIMENSION {
        return Err(PreviewApiError::TooLarge);
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_DIMENSION);
    limits.max_image_height = Some(MAX_SOURCE_DIMENSION);
    limits.max_alloc = Some(MAX_SOURCE_ALLOCATION);
    limits
        .reserve(decoder.total_bytes())
        .map_err(|_| PreviewApiError::TooLarge)?;
    decoder
        .set_limits(limits)
        .map_err(|_| PreviewApiError::TooLarge)?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut decoded =
        DynamicImage::from_decoder(decoder).map_err(|_| PreviewApiError::UnsupportedSource)?;
    decoded.apply_orientation(orientation);
    let rgb = decoded.to_rgb8();
    let pixels = rgb.pixels().map(|pixel| pixel.0).collect::<Vec<_>>();
    let oriented = orient_and_bound(
        RgbImage {
            width: rgb.width(),
            height: rgb.height(),
            pixels,
        },
        Orientation::Normal,
        preset.max_dimension(),
    )
    .map_err(|_| PreviewApiError::TooLarge)?;
    let raw = oriented
        .pixels
        .iter()
        .flat_map(|pixel| pixel.iter().copied())
        .collect();
    let image = image::RgbImage::from_raw(oriented.width, oriented.height, raw)
        .ok_or(PreviewApiError::Unavailable)?;
    let mut output = Vec::new();
    JpegEncoder::new_with_quality(&mut output, JPEG_QUALITY)
        .encode_image(&DynamicImage::ImageRgb8(image))
        .map_err(|_| PreviewApiError::Unavailable)?;
    if output.len() > MAX_PREVIEW_BYTES {
        return Err(PreviewApiError::TooLarge);
    }
    Ok(GeneratedPreview {
        width: oriented.width,
        height: oriented.height,
        preset,
        format: PreviewFormat::Jpeg,
        bytes: output,
        source_hash,
        render_version: crate::media::previews::PREVIEW_RENDER_VERSION.to_owned(),
    })
}

fn asset(preview: Arc<GeneratedPreview>) -> PreviewAsset {
    let etag = format!(
        "{}-{}-{}",
        preview.source_hash,
        preview.render_version,
        preview.preset.name()
    );
    PreviewAsset {
        bytes: Arc::new(preview.bytes.clone()),
        etag,
    }
}

fn hex(hash: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(hash.len() * 2);
    for byte in hash {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
