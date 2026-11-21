//! Non-destructive, bounded preview generation with versioned cache keys.
//!
//! The server adapter supplies a read-only decoder, cache, encoder, and worker
//! loop. Requests are queued by priority and coalesced by their complete cache
//! key. The worker receives decoded pixels only; this module never writes the
//! source path.

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};

pub const PREVIEW_RENDER_VERSION: &str = "preview-render-1";
pub const JPEG_QUALITY: u8 = 85;
pub const MAX_PREVIEW_DIMENSION: u32 = 3_200;
pub const MAX_DECODED_PIXELS: u64 = 16_000_000;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PreviewPreset {
    Small,
    Medium,
    Large,
    Full,
}

impl PreviewPreset {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
            Self::Full => "full",
        }
    }

    pub const fn max_dimension(self) -> u32 {
        match self {
            Self::Small => 256,
            Self::Medium => 800,
            Self::Large => 1_600,
            Self::Full => 3_200,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PreviewFormat {
    Jpeg,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Priority {
    Background,
    Visible,
}

impl Priority {
    fn rank(self) -> u8 {
        match self {
            Self::Background => 0,
            Self::Visible => 1,
        }
    }
}

/// EXIF orientation from the source metadata. The decoder returns raw pixels;
/// generation applies this orientation before calculating the output bounds.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum Orientation {
    #[default]
    Normal,
    MirrorHorizontal,
    Rotate180,
    MirrorVertical,
    Transpose,
    Rotate90,
    Transverse,
    Rotate270,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PreviewCacheKey {
    pub photo_identity: String,
    pub source_revision: String,
    pub original_hash: String,
    pub render_version: String,
    pub preset: PreviewPreset,
    pub max_dimension: u32,
    pub format: PreviewFormat,
    pub quality: u8,
    pub orientation: Orientation,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewRequest {
    pub photo_identity: String,
    pub source_path: PathBuf,
    /// Monotonic catalog revision covering source bytes and relevant metadata.
    pub source_revision: String,
    /// Hash of the original bytes; included in the key and never modified.
    pub original_hash: String,
    pub render_version: String,
    pub preset: PreviewPreset,
    pub format: PreviewFormat,
    pub quality: u8,
    pub orientation: Orientation,
    pub priority: Priority,
}

impl PreviewRequest {
    pub fn cache_key(&self) -> PreviewCacheKey {
        PreviewCacheKey {
            photo_identity: self.photo_identity.clone(),
            source_revision: self.source_revision.clone(),
            original_hash: self.original_hash.clone(),
            render_version: self.render_version.clone(),
            preset: self.preset,
            max_dimension: self.preset.max_dimension(),
            format: self.format,
            quality: self.quality,
            orientation: self.orientation,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RgbImage {
    pub width: u32,
    pub height: u32,
    /// Row-major RGB bytes. The decoder contract is checked before processing.
    pub pixels: Vec<[u8; 3]>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GeneratedPreview {
    pub width: u32,
    pub height: u32,
    pub preset: PreviewPreset,
    pub format: PreviewFormat,
    pub bytes: Vec<u8>,
    pub source_hash: String,
    pub render_version: String,
}

pub type PreviewResult = Result<Arc<GeneratedPreview>, PreviewError>;
pub type PreviewReceiver = mpsc::Receiver<PreviewResult>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PreviewError {
    Cache(String),
    Decode(String),
    Encode(String),
    InvalidDimensions,
    PixelBufferLength { expected: usize, actual: usize },
    ImageTooLarge { pixels: u64, maximum: u64 },
}

pub trait PreviewDecoder: Send + Sync {
    /// Decode source data read-only. Return raw, pre-orientation RGB pixels.
    fn decode(&self, path: &Path, max_pixels: u64) -> Result<RgbImage, String>;
}

pub trait PreviewEncoder: Send + Sync {
    /// Encode only the derived, bounded image. Implementations should use JPEG
    /// for `PreviewFormat::Jpeg` and honor the supplied quality.
    fn encode(
        &self,
        image: &RgbImage,
        format: PreviewFormat,
        quality: u8,
    ) -> Result<Vec<u8>, String>;
}

pub trait PreviewCache: Send + Sync {
    fn get(&self, key: &PreviewCacheKey) -> Result<Option<Arc<GeneratedPreview>>, String>;
    fn put(&self, key: PreviewCacheKey, preview: Arc<GeneratedPreview>) -> Result<(), String>;
}

struct QueuedJob {
    key: PreviewCacheKey,
    request: PreviewRequest,
    sequence: u64,
    waiters: Vec<mpsc::Sender<PreviewResult>>,
}

#[derive(Default)]
struct QueueState {
    next_sequence: u64,
    pending: VecDeque<QueuedJob>,
    active: HashMap<PreviewCacheKey, Vec<mpsc::Sender<PreviewResult>>>,
}

/// Thread-safe request coordinator. A server worker calls `process_next` until
/// it returns false. Visible jobs are selected first; ties preserve FIFO order.
/// Concurrent requests with an identical cache key share one pending or active
/// job and receive the same generated preview.
pub struct PreviewService<D, C, E> {
    decoder: D,
    cache: C,
    encoder: E,
    queue: Mutex<QueueState>,
}

impl<D: PreviewDecoder, C: PreviewCache, E: PreviewEncoder> PreviewService<D, C, E> {
    pub fn new(decoder: D, cache: C, encoder: E) -> Self {
        Self {
            decoder,
            cache,
            encoder,
            queue: Mutex::new(QueueState::default()),
        }
    }

    /// Return cached data immediately, otherwise enqueue or join a generation
    /// job. The returned receiver completes when a worker processes that job.
    pub fn submit(&self, request: PreviewRequest) -> Result<PreviewReceiver, PreviewError> {
        let key = request.cache_key();
        if let Some(hit) = self.cache.get(&key).map_err(PreviewError::Cache)? {
            let (sender, receiver) = mpsc::channel();
            let _ = sender.send(Ok(hit));
            return Ok(receiver);
        }

        let (sender, receiver) = mpsc::channel();
        let mut queue = self.queue.lock().expect("preview queue mutex poisoned");
        // Close the race where another worker populated the cache after the
        // fast lookup but before this request acquired the queue lock.
        if let Some(hit) = self.cache.get(&key).map_err(PreviewError::Cache)? {
            let _ = sender.send(Ok(hit));
            return Ok(receiver);
        }
        if let Some(job) = queue.pending.iter_mut().find(|job| job.key == key) {
            job.waiters.push(sender);
            if request.priority.rank() > job.request.priority.rank() {
                job.request.priority = request.priority;
            }
            return Ok(receiver);
        }
        if let Some(waiters) = queue.active.get_mut(&key) {
            waiters.push(sender);
            return Ok(receiver);
        }

        let sequence = queue.next_sequence;
        queue.next_sequence = queue.next_sequence.wrapping_add(1);
        queue.pending.push_back(QueuedJob {
            key,
            request,
            sequence,
            waiters: vec![sender],
        });
        Ok(receiver)
    }

    /// Process the next job according to priority. Returns false when no work
    /// is queued. Generation errors are delivered to every joined requester.
    pub fn process_next(&self) -> bool {
        let mut job = {
            let mut queue = self.queue.lock().expect("preview queue mutex poisoned");
            let Some(index) = queue
                .pending
                .iter()
                .enumerate()
                .max_by(|(_, left), (_, right)| {
                    left.request
                        .priority
                        .rank()
                        .cmp(&right.request.priority.rank())
                        .then_with(|| right.sequence.cmp(&left.sequence))
                })
                .map(|(index, _)| index)
            else {
                return false;
            };
            let job = queue.pending.remove(index).expect("selected preview job");
            queue.active.insert(job.key.clone(), Vec::new());
            job
        };

        let result = self.generate(&job.request, &job.key);
        let joined = {
            let mut queue = self.queue.lock().expect("preview queue mutex poisoned");
            queue.active.remove(&job.key).unwrap_or_default()
        };
        job.waiters.extend(joined);
        for waiter in job.waiters {
            let _ = waiter.send(result.clone());
        }
        true
    }

    fn generate(&self, request: &PreviewRequest, key: &PreviewCacheKey) -> PreviewResult {
        let decoded = self
            .decoder
            .decode(&request.source_path, MAX_DECODED_PIXELS)
            .map_err(PreviewError::Decode)?;
        let oriented = orient_and_bound(decoded, request.orientation, key.max_dimension)?;
        let bytes = self
            .encoder
            .encode(&oriented, key.format, key.quality)
            .map_err(PreviewError::Encode)?;
        let preview = Arc::new(GeneratedPreview {
            width: oriented.width,
            height: oriented.height,
            preset: key.preset,
            format: key.format,
            bytes,
            source_hash: request.original_hash.clone(),
            render_version: request.render_version.clone(),
        });
        self.cache
            .put(key.clone(), preview.clone())
            .map_err(PreviewError::Cache)?;
        Ok(preview)
    }

    #[cfg(test)]
    fn pending_count(&self) -> usize {
        self.queue
            .lock()
            .expect("preview queue mutex poisoned")
            .pending
            .len()
    }
}

/// Apply all eight EXIF orientations, then downsample without upscaling while
/// preserving aspect ratio and the requested longest-side bound.
pub fn orient_and_bound(
    image: RgbImage,
    orientation: Orientation,
    max_dimension: u32,
) -> Result<RgbImage, PreviewError> {
    if image.width == 0 || image.height == 0 || max_dimension == 0 {
        return Err(PreviewError::InvalidDimensions);
    }
    let max_dimension = max_dimension.min(MAX_PREVIEW_DIMENSION);
    let expected = u64::from(image.width)
        .checked_mul(u64::from(image.height))
        .ok_or(PreviewError::InvalidDimensions)?;
    if expected > MAX_DECODED_PIXELS {
        return Err(PreviewError::ImageTooLarge {
            pixels: expected,
            maximum: MAX_DECODED_PIXELS,
        });
    }
    if expected != image.pixels.len() as u64 {
        return Err(PreviewError::PixelBufferLength {
            expected: usize::try_from(expected).unwrap_or(usize::MAX),
            actual: image.pixels.len(),
        });
    }

    let (oriented_width, oriented_height) = match orientation {
        Orientation::Transpose
        | Orientation::Rotate90
        | Orientation::Transverse
        | Orientation::Rotate270 => (image.height, image.width),
        _ => (image.width, image.height),
    };
    let longest = oriented_width.max(oriented_height);
    let (width, height) = if longest <= max_dimension {
        (oriented_width, oriented_height)
    } else if oriented_width >= oriented_height {
        (
            max_dimension,
            ((u64::from(oriented_height) * u64::from(max_dimension)) / u64::from(oriented_width))
                .max(1) as u32,
        )
    } else {
        (
            ((u64::from(oriented_width) * u64::from(max_dimension)) / u64::from(oriented_height))
                .max(1) as u32,
            max_dimension,
        )
    };
    let mut pixels = Vec::with_capacity((width as usize).saturating_mul(height as usize));
    for y in 0..height {
        for x in 0..width {
            // Map output pixel centers to source coordinates, as conventional
            // bilinear resamplers do. This avoids nearest-neighbor aliasing and
            // preserves smooth thumbnail transitions while downsampling.
            let source_x = (((f64::from(x) + 0.5) * f64::from(oriented_width) / f64::from(width))
                - 0.5)
                .clamp(0.0, f64::from(oriented_width - 1));
            let source_y =
                (((f64::from(y) + 0.5) * f64::from(oriented_height) / f64::from(height)) - 0.5)
                    .clamp(0.0, f64::from(oriented_height - 1));
            let x0 = source_x.floor() as u32;
            let y0 = source_y.floor() as u32;
            let x1 = (x0 + 1).min(oriented_width - 1);
            let y1 = (y0 + 1).min(oriented_height - 1);
            let x_weight = source_x - f64::from(x0);
            let y_weight = source_y - f64::from(y0);
            let top_left = oriented_pixel_at(&image, x0, y0, orientation);
            let top_right = oriented_pixel_at(&image, x1, y0, orientation);
            let bottom_left = oriented_pixel_at(&image, x0, y1, orientation);
            let bottom_right = oriented_pixel_at(&image, x1, y1, orientation);
            let mut pixel = [0_u8; 3];
            for channel in 0..3 {
                let top = f64::from(top_left[channel]) * (1.0 - x_weight)
                    + f64::from(top_right[channel]) * x_weight;
                let bottom = f64::from(bottom_left[channel]) * (1.0 - x_weight)
                    + f64::from(bottom_right[channel]) * x_weight;
                pixel[channel] = (top * (1.0 - y_weight) + bottom * y_weight)
                    .round()
                    .clamp(0.0, 255.0) as u8;
            }
            pixels.push(pixel);
        }
    }
    Ok(RgbImage {
        width,
        height,
        pixels,
    })
}

fn oriented_pixel_at(image: &RgbImage, x: u32, y: u32, orientation: Orientation) -> [u8; 3] {
    let (source_x, source_y) = map_oriented_to_source(x, y, image.width, image.height, orientation);
    image.pixels[(source_y * image.width + source_x) as usize]
}

fn map_oriented_to_source(
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    orientation: Orientation,
) -> (u32, u32) {
    match orientation {
        Orientation::Normal => (x, y),
        Orientation::MirrorHorizontal => (width - 1 - x, y),
        Orientation::Rotate180 => (width - 1 - x, height - 1 - y),
        Orientation::MirrorVertical => (x, height - 1 - y),
        Orientation::Transpose => (y, x),
        Orientation::Rotate90 => (y, height - 1 - x),
        Orientation::Transverse => (width - 1 - y, height - 1 - x),
        Orientation::Rotate270 => (width - 1 - y, x),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct MemoryCache(Mutex<HashMap<PreviewCacheKey, Arc<GeneratedPreview>>>);

    impl PreviewCache for MemoryCache {
        fn get(&self, key: &PreviewCacheKey) -> Result<Option<Arc<GeneratedPreview>>, String> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }

        fn put(&self, key: PreviewCacheKey, preview: Arc<GeneratedPreview>) -> Result<(), String> {
            self.0.lock().unwrap().insert(key, preview);
            Ok(())
        }
    }

    struct FakeDecoder {
        image: RgbImage,
        calls: AtomicUsize,
    }

    impl PreviewDecoder for FakeDecoder {
        fn decode(&self, _path: &Path, max_pixels: u64) -> Result<RgbImage, String> {
            assert!(u64::from(self.image.width) * u64::from(self.image.height) <= max_pixels);
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(self.image.clone())
        }
    }

    #[derive(Default)]
    struct FakeEncoder;

    impl PreviewEncoder for FakeEncoder {
        fn encode(
            &self,
            image: &RgbImage,
            format: PreviewFormat,
            quality: u8,
        ) -> Result<Vec<u8>, String> {
            assert_eq!(format, PreviewFormat::Jpeg);
            assert_eq!(quality, JPEG_QUALITY);
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&image.width.to_le_bytes());
            bytes.extend_from_slice(&image.height.to_le_bytes());
            for pixel in &image.pixels {
                bytes.extend_from_slice(pixel);
            }
            Ok(bytes)
        }
    }

    fn request(id: &str, priority: Priority) -> PreviewRequest {
        PreviewRequest {
            photo_identity: id.into(),
            source_path: PathBuf::from("original.jpg"),
            source_revision: "rev-1".into(),
            original_hash: "sha256-original".into(),
            render_version: PREVIEW_RENDER_VERSION.into(),
            preset: PreviewPreset::Medium,
            format: PreviewFormat::Jpeg,
            quality: JPEG_QUALITY,
            orientation: Orientation::Normal,
            priority,
        }
    }

    fn service(image: RgbImage) -> PreviewService<FakeDecoder, MemoryCache, FakeEncoder> {
        PreviewService::new(
            FakeDecoder {
                image,
                calls: AtomicUsize::new(0),
            },
            MemoryCache::default(),
            FakeEncoder,
        )
    }

    #[test]
    fn versioned_key_invalidates_for_identity_source_hash_revision_render_and_variant() {
        let original = request("photo-a", Priority::Visible);
        let key = original.cache_key();
        let mut changed = original.clone();
        changed.photo_identity = "photo-b".into();
        assert_ne!(key, changed.cache_key());
        changed = original.clone();
        changed.source_revision = "rev-2".into();
        assert_ne!(key, changed.cache_key());
        changed = original.clone();
        changed.original_hash = "sha256-new".into();
        assert_ne!(key, changed.cache_key());
        changed = original.clone();
        changed.render_version = "preview-render-2".into();
        assert_ne!(key, changed.cache_key());
        changed = original.clone();
        changed.preset = PreviewPreset::Large;
        assert_ne!(key, changed.cache_key());
        changed = original.clone();
        changed.quality = 90;
        assert_ne!(key, changed.cache_key());
    }

    #[test]
    fn output_is_bounded_oriented_and_preserves_aspect_ratio() {
        let pixels = (0..6).map(|value| [value, 0, 0]).collect();
        let raw = RgbImage {
            width: 3,
            height: 2,
            pixels,
        };
        let rotated = orient_and_bound(raw, Orientation::Rotate90, 3_200).unwrap();
        assert_eq!((rotated.width, rotated.height), (2, 3));
        assert_eq!(
            rotated.pixels,
            vec![
                [3, 0, 0],
                [0, 0, 0],
                [4, 0, 0],
                [1, 0, 0],
                [5, 0, 0],
                [2, 0, 0]
            ]
        );

        let large = RgbImage {
            width: 4_000,
            height: 2_000,
            pixels: vec![[8, 9, 10]; 8_000_000],
        };
        let bounded = orient_and_bound(
            large,
            Orientation::Normal,
            PreviewPreset::Full.max_dimension(),
        )
        .unwrap();
        assert_eq!((bounded.width, bounded.height), (3_200, 1_600));
        assert!(bounded.width.max(bounded.height) <= MAX_PREVIEW_DIMENSION);
    }

    #[test]
    fn every_exif_orientation_maps_pixels_deterministically() {
        let raw = RgbImage {
            width: 3,
            height: 2,
            pixels: (0..6).map(|value| [value, 0, 0]).collect(),
        };
        let cases = [
            (Orientation::Normal, 3, 2, vec![0, 1, 2, 3, 4, 5]),
            (Orientation::MirrorHorizontal, 3, 2, vec![2, 1, 0, 5, 4, 3]),
            (Orientation::Rotate180, 3, 2, vec![5, 4, 3, 2, 1, 0]),
            (Orientation::MirrorVertical, 3, 2, vec![3, 4, 5, 0, 1, 2]),
            (Orientation::Transpose, 2, 3, vec![0, 3, 1, 4, 2, 5]),
            (Orientation::Rotate90, 2, 3, vec![3, 0, 4, 1, 5, 2]),
            (Orientation::Transverse, 2, 3, vec![5, 2, 4, 1, 3, 0]),
            (Orientation::Rotate270, 2, 3, vec![2, 5, 1, 4, 0, 3]),
        ];

        for (orientation, width, height, expected) in cases {
            let output = orient_and_bound(raw.clone(), orientation, 3_200).unwrap();
            assert_eq!(
                (output.width, output.height),
                (width, height),
                "{orientation:?}"
            );
            assert_eq!(
                output
                    .pixels
                    .iter()
                    .map(|pixel| pixel[0])
                    .collect::<Vec<_>>(),
                expected,
                "{orientation:?}"
            );
        }
    }

    #[test]
    fn downsampling_uses_deterministic_bilinear_interpolation() {
        let image = RgbImage {
            width: 4,
            height: 2,
            pixels: vec![
                [0, 0, 0],
                [40, 40, 40],
                [80, 80, 80],
                [120, 120, 120],
                [40, 40, 40],
                [80, 80, 80],
                [120, 120, 120],
                [160, 160, 160],
            ],
        };
        let output = orient_and_bound(image, Orientation::Normal, 2).unwrap();
        assert_eq!((output.width, output.height), (2, 1));
        assert_eq!(output.pixels, vec![[40, 40, 40], [120, 120, 120]]);
    }

    #[test]
    fn visible_requests_are_selected_before_background_jobs() {
        let service = service(RgbImage {
            width: 1,
            height: 1,
            pixels: vec![[20, 30, 40]],
        });
        let background = service
            .submit(request("background", Priority::Background))
            .unwrap();
        let visible = service
            .submit(request("visible", Priority::Visible))
            .unwrap();
        assert_eq!(service.pending_count(), 2);
        assert!(service.process_next());
        let visible_result = visible.recv().unwrap().unwrap();
        assert_eq!(visible_result.bytes[8..], [20, 30, 40]);
        assert!(matches!(
            background.try_recv(),
            Err(mpsc::TryRecvError::Empty)
        ));
        assert!(service.process_next());
        assert!(background.recv().unwrap().is_ok());
        assert!(!service.process_next());
    }

    #[test]
    fn duplicate_concurrent_submissions_share_one_render_and_do_not_change_source_hash() {
        let service = Arc::new(service(RgbImage {
            width: 2,
            height: 1,
            pixels: vec![[1, 2, 3], [4, 5, 6]],
        }));
        let original_hash = "sha256-original".to_owned();
        let request_a = request("photo-a", Priority::Background);
        let request_b = request("photo-a", Priority::Visible);
        let (receiver_a, receiver_b) = std::thread::scope(|scope| {
            let service_a = service.clone();
            let service_b = service.clone();
            let first = scope.spawn(move || service_a.submit(request_a).unwrap());
            let second = scope.spawn(move || service_b.submit(request_b).unwrap());
            (first.join().unwrap(), second.join().unwrap())
        });
        assert_eq!(service.pending_count(), 1);
        assert!(service.process_next());
        let preview_a = receiver_a.recv().unwrap().unwrap();
        let preview_b = receiver_b.recv().unwrap().unwrap();
        assert!(Arc::ptr_eq(&preview_a, &preview_b));
        assert_eq!(preview_a.source_hash, original_hash);
        assert_eq!(service.decoder.calls.load(Ordering::SeqCst), 1);

        let cached = service
            .submit(request("photo-a", Priority::Visible))
            .unwrap();
        assert!(Arc::ptr_eq(&preview_a, &cached.recv().unwrap().unwrap()));
        assert_eq!(service.decoder.calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn empty_and_inconsistent_images_fail_without_panicking() {
        assert_eq!(
            orient_and_bound(
                RgbImage {
                    width: 0,
                    height: 1,
                    pixels: vec![],
                },
                Orientation::Normal,
                256,
            ),
            Err(PreviewError::InvalidDimensions)
        );
        assert!(matches!(
            orient_and_bound(
                RgbImage {
                    width: 2,
                    height: 2,
                    pixels: vec![[0, 0, 0]],
                },
                Orientation::Normal,
                256,
            ),
            Err(PreviewError::PixelBufferLength { .. })
        ));
    }
}
