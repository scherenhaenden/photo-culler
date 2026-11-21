//! Async HTTP read adapter over the legacy Photo Culler SQLite catalog.
//!
//! This module keeps SQLx and on-disk media details behind the server's stable
//! catalog-read contract. File paths are used internally to produce bounded
//! previews and are never returned in HTTP DTOs.

use std::{
    io::{Cursor, Read},
    path::PathBuf,
};

use image::{
    DynamicImage, ImageDecoder, ImageReader, codecs::jpeg::JpegEncoder, metadata::Orientation,
};
use sqlx::Row;

use crate::http::catalog_reads::{
    Catalog as HttpCatalog, CatalogFuture, Error, Filter, Gallery, Group, Photo, Profile, Session,
    Summary, Thumbnail,
};
use crate::http::catalog_writes::decision::{
    DecisionFuture, DecisionReceipt, DecisionRequest, DecisionWriteError, DecisionWriter,
};

pub struct SqliteHttpCatalog {
    catalog: super::Catalog,
    active_gallery_id: Option<String>,
}

impl Clone for SqliteHttpCatalog {
    fn clone(&self) -> Self {
        Self {
            catalog: super::Catalog {
                writer: self.catalog.writer.clone(),
                readers: self.catalog.readers.clone(),
            },
            active_gallery_id: self.active_gallery_id.clone(),
        }
    }
}

impl SqliteHttpCatalog {
    pub fn new(catalog: super::Catalog) -> Self {
        Self {
            catalog,
            active_gallery_id: None,
        }
    }

    /// Select the gallery shown by catalog routes when import configuration
    /// explicitly chose a destination. `None` keeps the legacy first-gallery
    /// behavior.
    pub fn with_active_gallery(mut self, gallery_id: Option<String>) -> Self {
        self.active_gallery_id = gallery_id;
        self
    }
}

impl HttpCatalog for SqliteHttpCatalog {
    fn galleries<'a>(&'a self) -> CatalogFuture<'a, Result<Vec<Gallery>, Error>> {
        Box::pin(async move {
            self.catalog
                .list_galleries()
                .await
                .map(|items| {
                    items
                        .into_iter()
                        .map(|(id, name)| Gallery { id, name })
                        .collect()
                })
                .map_err(map_db_error)
        })
    }

    fn active_gallery<'a>(&'a self) -> CatalogFuture<'a, Result<Option<String>, Error>> {
        Box::pin(async move {
            if let Some(gallery_id) = &self.active_gallery_id {
                return Ok(Some(gallery_id.clone()));
            }
            // The legacy catalog has no active-gallery setting. Match the
            // existing Python Library behavior: use the first created gallery.
            sqlx::query_scalar::<_, String>(
                "SELECT id FROM galleries ORDER BY created_at, id LIMIT 1",
            )
            .fetch_optional(&self.catalog.readers)
            .await
            .map_err(map_db_error)
        })
    }

    fn photos<'a>(
        &'a self,
        filter: &'a Filter,
        after_id: Option<&'a str>,
        limit: usize,
    ) -> CatalogFuture<'a, Result<Vec<Photo>, Error>> {
        Box::pin(async move {
            self.catalog
                .list_gallery_filtered(
                    &filter.gallery_id,
                    filter.decision.as_deref(),
                    after_id,
                    limit,
                )
                .await
                .map(|rows| rows.into_iter().map(to_http_photo).collect())
                .map_err(map_db_error)
        })
    }

    fn photo<'a>(&'a self, id: &'a str) -> CatalogFuture<'a, Result<Option<Photo>, Error>> {
        Box::pin(async move {
            self.catalog
                .get(id)
                .await
                .map(|item| item.map(to_http_photo))
                .map_err(map_db_error)
        })
    }

    fn sessions<'a>(
        &'a self,
        gallery_id: &'a str,
    ) -> CatalogFuture<'a, Result<Vec<Session>, Error>> {
        Box::pin(async move {
            sqlx::query(
                "SELECT p.session_id, COALESCE(s.name, p.session_id) AS name, COUNT(*) AS photo_count FROM photos p LEFT JOIN sessions s ON s.session_id = p.session_id WHERE p.gallery_id = ? AND p.session_id IS NOT NULL GROUP BY p.session_id, s.name ORDER BY p.session_id",
            )
            .bind(gallery_id)
            .fetch_all(&self.catalog.readers)
            .await
            .map_err(map_db_error)?
            .into_iter()
            .map(|row| {
                Ok(Session {
                    id: row.try_get("session_id").map_err(map_db_error)?,
                    name: row.try_get("name").map_err(map_db_error)?,
                    photo_count: row
                        .try_get::<i64, _>("photo_count")
                        .map_err(map_db_error)?
                        .max(0) as u64,
                })
            })
            .collect()
        })
    }

    fn groups<'a>(&'a self, gallery_id: &'a str) -> CatalogFuture<'a, Result<Vec<Group>, Error>> {
        Box::pin(async move {
            sqlx::query(
                "SELECT burst_id, COUNT(*) AS photo_count, (SELECT p2.photo_id FROM photos p2 WHERE p2.gallery_id = p.gallery_id AND p2.burst_id = p.burst_id ORDER BY COALESCE(p2.score, 0) DESC, p2.photo_id ASC LIMIT 1) AS recommended_photo_id FROM photos p WHERE p.gallery_id = ? AND p.burst_id IS NOT NULL GROUP BY burst_id ORDER BY burst_id",
            )
            .bind(gallery_id)
            .fetch_all(&self.catalog.readers)
            .await
            .map_err(map_db_error)?
            .into_iter()
            .map(|row| {
                Ok(Group {
                    id: row.try_get("burst_id").map_err(map_db_error)?,
                    photo_count: row
                        .try_get::<i64, _>("photo_count")
                        .map_err(map_db_error)?
                        .max(0) as u64,
                    recommended_photo_id: row
                        .try_get("recommended_photo_id")
                        .map_err(map_db_error)?,
                })
            })
            .collect()
        })
    }

    fn profiles<'a>(&'a self) -> CatalogFuture<'a, Result<Vec<Profile>, Error>> {
        // Analysis profiles are currently stored in a separate JSON settings
        // file by the legacy app, not in the SQLite schema. Do not invent rows.
        Box::pin(async { Ok(Vec::new()) })
    }

    fn summary<'a>(&'a self, gallery_id: &'a str) -> CatalogFuture<'a, Result<Summary, Error>> {
        Box::pin(async move {
            // Keep all aggregates in the same read snapshot while imports or
            // decision writes may update the catalog concurrently.
            let mut tx = self.catalog.readers.begin().await.map_err(map_db_error)?;
            let counts = sqlx::query(
                "SELECT COUNT(*) AS total_photos, SUM(CASE WHEN UPPER(decision) IN ('BEST','KEEP') THEN 1 ELSE 0 END) AS selected_count, SUM(CASE WHEN decision IS NULL OR UPPER(decision) IN ('REVIEW','UNPROCESSED') THEN 1 ELSE 0 END) AS pending_count FROM photos WHERE gallery_id = ?",
            )
            .bind(gallery_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_db_error)?;
            let total_files: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM files f JOIN photos p ON p.id = f.photo_id WHERE p.gallery_id = ?",
            )
            .bind(gallery_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(map_db_error)?;
            let decisions = sqlx::query(
                "SELECT COALESCE(decision, 'UNPROCESSED') AS decision, COUNT(*) AS count FROM photos WHERE gallery_id = ? GROUP BY COALESCE(decision, 'UNPROCESSED') ORDER BY decision",
            )
            .bind(gallery_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(map_db_error)?
            .into_iter()
            .map(|row| {
                Ok((
                    row.try_get("decision").map_err(map_db_error)?,
                    row.try_get::<i64, _>("count")
                        .map_err(map_db_error)?
                        .max(0) as u64,
                ))
            })
            .collect::<Result<std::collections::BTreeMap<_, _>, Error>>()?;
            let tiers = sqlx::query(
                "SELECT COALESCE(quality_tier, 'unknown') AS tier, COUNT(*) AS count FROM photos WHERE gallery_id = ? GROUP BY COALESCE(quality_tier, 'unknown') ORDER BY tier",
            )
            .bind(gallery_id)
            .fetch_all(&mut *tx)
            .await
            .map_err(map_db_error)?
            .into_iter()
            .map(|row| {
                Ok((
                    row.try_get("tier").map_err(map_db_error)?,
                    row.try_get::<i64, _>("count")
                        .map_err(map_db_error)?
                        .max(0) as u64,
                ))
            })
            .collect::<Result<std::collections::BTreeMap<_, _>, Error>>()?;

            tx.commit().await.map_err(map_db_error)?;

            Ok(Summary {
                total_photos: counts
                    .try_get::<i64, _>("total_photos")
                    .map_err(map_db_error)?
                    .max(0) as u64,
                total_files: total_files.max(0) as u64,
                selected_count: counts
                    .try_get::<Option<i64>, _>("selected_count")
                    .map_err(map_db_error)?
                    .unwrap_or_default()
                    .max(0) as u64,
                pending_count: counts
                    .try_get::<Option<i64>, _>("pending_count")
                    .map_err(map_db_error)?
                    .unwrap_or_default()
                    .max(0) as u64,
                decisions,
                quality_tiers: tiers,
            })
        })
    }

    fn thumbnail<'a>(
        &'a self,
        photo_id: &'a str,
        size: u32,
    ) -> CatalogFuture<'a, Result<Option<Thumbnail>, Error>> {
        Box::pin(async move {
            let paths = self
                .catalog
                .display_paths(photo_id)
                .await
                .map_err(map_db_error)?;
            if paths.is_empty() {
                return Ok(None);
            }
            let rendered = tokio::task::spawn_blocking(move || {
                paths
                    .into_iter()
                    .find_map(|path| render_thumbnail(path, size).ok())
            })
            .await
            .map_err(|_| Error::Unavailable)?
            .ok_or(Error::Unavailable)?;
            Ok(Some(Thumbnail {
                content_type: "image/jpeg".into(),
                bytes: rendered,
            }))
        })
    }
}

impl DecisionWriter for SqliteHttpCatalog {
    fn write_decision<'a>(
        &'a self,
        photo_id: &'a str,
        request: &'a DecisionRequest,
    ) -> DecisionFuture<'a, Result<DecisionReceipt, DecisionWriteError>> {
        Box::pin(async move {
            if photo_id.trim().is_empty()
                || photo_id.len() > 256
                || request.request_id.trim().is_empty()
                || request.request_id.trim().len() > 128
            {
                return Err(DecisionWriteError::InvalidRequest);
            }
            let expected_revision = i64::try_from(request.expected_revision)
                .map_err(|_| DecisionWriteError::InvalidRequest)?;
            let request_id = request.request_id.trim();
            let decision = request.decision.database_value();

            // Reserve SQLite's single writer before reading the revision so
            // two concurrent requests cannot both pass the same expected value.
            let mut connection = self
                .catalog
                .writer
                .acquire()
                .await
                .map_err(|_| DecisionWriteError::Unavailable)?;
            sqlx::query("BEGIN IMMEDIATE")
                .execute(&mut *connection)
                .await
                .map_err(|_| DecisionWriteError::Unavailable)?;

            let result = async {
                sqlx::query(
                    "CREATE TABLE IF NOT EXISTS photo_decision_revisions (photo_id TEXT PRIMARY KEY REFERENCES photos(photo_id), revision INTEGER NOT NULL CHECK(revision >= 0))",
                )
                .execute(&mut *connection)
                .await
                .map_err(|_| DecisionWriteError::Unavailable)?;
                sqlx::query(
                    "CREATE TABLE IF NOT EXISTS decision_write_receipts (request_id TEXT PRIMARY KEY, photo_id TEXT NOT NULL, expected_revision INTEGER NOT NULL, decision TEXT NOT NULL, revision INTEGER NOT NULL, created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP)",
                )
                .execute(&mut *connection)
                .await
                .map_err(|_| DecisionWriteError::Unavailable)?;

                let previous = sqlx::query(
                    "SELECT photo_id, expected_revision, decision, revision FROM decision_write_receipts WHERE request_id = ?",
                )
                .bind(request_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(|_| DecisionWriteError::Unavailable)?;
                if let Some(previous) = previous {
                    let same = previous
                        .try_get::<String, _>("photo_id")
                        .map_err(|_| DecisionWriteError::Unavailable)?
                        == photo_id
                        && previous
                            .try_get::<i64, _>("expected_revision")
                            .map_err(|_| DecisionWriteError::Unavailable)?
                            == expected_revision
                        && previous
                            .try_get::<String, _>("decision")
                            .map_err(|_| DecisionWriteError::Unavailable)?
                            == decision;
                    if !same {
                        return Err(DecisionWriteError::InvalidState);
                    }
                    let revision: i64 = previous
                        .try_get("revision")
                        .map_err(|_| DecisionWriteError::Unavailable)?;
                    return Ok(DecisionReceipt {
                        contract_version: 1,
                        photo_id: photo_id.to_owned(),
                        decision: request.decision,
                        revision: revision.max(0) as u64,
                        request_id: request_id.to_owned(),
                    });
                }

                let exists: i64 = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM photos WHERE photo_id = ?)",
                )
                .bind(photo_id)
                .fetch_one(&mut *connection)
                .await
                .map_err(|_| DecisionWriteError::Unavailable)?;
                if exists == 0 {
                    return Err(DecisionWriteError::NotFound);
                }

                let revision: Option<i64> = sqlx::query_scalar(
                    "SELECT revision FROM photo_decision_revisions WHERE photo_id = ?",
                )
                .bind(photo_id)
                .fetch_optional(&mut *connection)
                .await
                .map_err(|_| DecisionWriteError::Unavailable)?;
                let current_revision = revision.unwrap_or(0);
                if current_revision != expected_revision {
                    return Err(DecisionWriteError::VersionConflict);
                }
                let next_revision = current_revision
                    .checked_add(1)
                    .ok_or(DecisionWriteError::Unavailable)?;

                let updated = sqlx::query("UPDATE photos SET decision = ? WHERE photo_id = ?")
                    .bind(decision)
                    .bind(photo_id)
                    .execute(&mut *connection)
                    .await
                    .map_err(|_| DecisionWriteError::Unavailable)?;
                if updated.rows_affected() != 1 {
                    return Err(DecisionWriteError::NotFound);
                }
                sqlx::query(
                    "INSERT INTO photo_decision_revisions(photo_id, revision) VALUES (?, ?) ON CONFLICT(photo_id) DO UPDATE SET revision = excluded.revision",
                )
                .bind(photo_id)
                .bind(next_revision)
                .execute(&mut *connection)
                .await
                .map_err(|_| DecisionWriteError::Unavailable)?;
                sqlx::query(
                    "INSERT INTO decision_write_receipts(request_id, photo_id, expected_revision, decision, revision) VALUES (?, ?, ?, ?, ?)",
                )
                .bind(request_id)
                .bind(photo_id)
                .bind(expected_revision)
                .bind(decision)
                .bind(next_revision)
                .execute(&mut *connection)
                .await
                .map_err(|_| DecisionWriteError::Unavailable)?;

                Ok(DecisionReceipt {
                    contract_version: 1,
                    photo_id: photo_id.to_owned(),
                    decision: request.decision,
                    revision: next_revision as u64,
                    request_id: request_id.to_owned(),
                })
            }
            .await;

            match result {
                Ok(receipt) => {
                    match sqlx::query("COMMIT").execute(&mut *connection).await {
                        Ok(_) => Ok(receipt),
                        Err(_) => {
                            // COMMIT may have left the raw-SQL transaction in
                            // an uncertain state. Never return this connection
                            // to the pool for another request to reuse.
                            connection.close_on_drop();
                            Err(DecisionWriteError::Unavailable)
                        }
                    }
                }
                Err(error) => {
                    if sqlx::query("ROLLBACK")
                        .execute(&mut *connection)
                        .await
                        .is_err()
                    {
                        // A failed rollback also makes transaction state
                        // uncertain, so discard the acquired connection.
                        connection.close_on_drop();
                    }
                    Err(error)
                }
            }
        })
    }
}

fn to_http_photo(photo: super::Photo) -> Photo {
    let decision = decision_name(photo.decision.as_deref());
    let metadata = photo
        .metadata
        .map(|metadata| {
            serde_json::json!({
                "capture_time": metadata.capture_time,
                "subsecond": metadata.subsecond,
                "camera_make": metadata.camera_make,
                "camera_model": metadata.camera_model,
                "serial_number": metadata.serial_number,
                "lens": metadata.lens,
                "iso": metadata.iso,
                "aperture": metadata.aperture,
                "shutter_speed": metadata.shutter_speed,
                "focal_length": metadata.focal_length,
                "orientation": metadata.orientation,
                "latitude": metadata.latitude,
                "longitude": metadata.longitude,
            })
        })
        .unwrap_or_else(|| serde_json::json!({}));
    let analysis_summary = serde_json::from_str(&photo.analysis_summary_json)
        .unwrap_or_else(|_| serde_json::json!({}));
    Photo {
        id: photo.photo_id,
        gallery_id: photo.gallery_id.unwrap_or_default(),
        name: photo.stem_name,
        decision,
        decision_revision: photo.decision_revision,
        score: photo.score,
        quality_tier: photo.quality_tier,
        analysis_summary,
        metadata,
    }
}

fn decision_name(decision: Option<&str>) -> String {
    match decision
        .unwrap_or("UNPROCESSED")
        .to_ascii_uppercase()
        .as_str()
    {
        "KEEP" | "BEST" | "PROTECTED_BY_COVERAGE" => "keep",
        "ALT" | "ALTERNATE" => "alt",
        "REJECT_TECHNICAL" | "REJECT_REDUNDANT" | "REJECT" => "reject",
        _ => "undecided",
    }
    .to_owned()
}

fn map_db_error(_: sqlx::Error) -> Error {
    Error::Unavailable
}

fn render_thumbnail(path: PathBuf, size: u32) -> Result<Vec<u8>, image::ImageError> {
    const MAX_SOURCE_BYTES: u64 = 128 * 1024 * 1024;
    const MAX_SOURCE_PIXELS: u64 = 64_000_000;

    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(MAX_SOURCE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err(image::ImageError::IoError(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "source image exceeds the preview input limit",
        )));
    }

    let reader = ImageReader::new(Cursor::new(bytes.as_slice())).with_guessed_format()?;
    let (width, height) = reader.into_dimensions()?;
    let pixels = u64::from(width)
        .checked_mul(u64::from(height))
        .ok_or_else(|| {
            image::ImageError::IoError(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "image dimensions overflow the preview limit",
            ))
        })?;
    if width == 0 || height == 0 || pixels > MAX_SOURCE_PIXELS {
        return Err(image::ImageError::IoError(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "image dimensions exceed the preview limit",
        )));
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(32_768);
    limits.max_image_height = Some(32_768);
    limits.max_alloc = Some(256 * 1024 * 1024);
    let mut reader = ImageReader::new(Cursor::new(bytes.as_slice())).with_guessed_format()?;
    reader.limits(limits.clone());
    let mut decoder = reader.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    // Match ImageReader::decode: reserve the output image before passing the
    // remaining allocation budget to the decoder. from_decoder alone does not
    // perform this reservation.
    limits.reserve(decoder.total_bytes())?;
    decoder.set_limits(limits)?;
    let mut image = DynamicImage::from_decoder(decoder)?;
    image.apply_orientation(orientation);
    let thumbnail = image.thumbnail(size, size);
    let mut output = Vec::new();
    JpegEncoder::new_with_quality(&mut output, 85).encode_image(&thumbnail)?;
    Ok(output)
}
