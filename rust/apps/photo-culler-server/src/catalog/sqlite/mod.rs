//! SQLite access for the legacy catalogue schema. Schema creation/migration is owned by task 18.
use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{path::Path, time::Duration};

/// One instance per catalog is the service's sole writer. Other processes must not write.
/// Readers use separate connections and see the last committed WAL snapshot.
pub struct Catalog {
    writer: SqlitePool,
    readers: SqlitePool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Photo {
    pub photo_id: String,
    pub gallery_id: Option<String>,
    pub stem_name: String,
    pub perceptual_hash: Option<String>,
    pub session_id: Option<String>,
    pub burst_id: Option<String>,
    pub decision: Option<String>,
    pub score: Option<f64>,
    pub quality_tier: Option<String>,
    pub analysis_summary_json: String,
    pub created_at: Option<String>,
    pub files: Vec<File>,
    pub metadata: Option<Metadata>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub volume_id: Option<i64>,
    pub import_source_id: Option<String>,
    pub last_seen_revision_id: Option<String>,
    pub relative_path: String,
    pub source_relative_path: Option<String>,
    pub status: String,
    pub role: String,
    pub size_bytes: i64,
    pub modified_time: f64,
    pub quick_hash: Option<String>,
    pub full_hash: Option<String>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Metadata {
    pub capture_time: Option<String>,
    pub subsecond: Option<String>,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub serial_number: Option<String>,
    pub lens: Option<String>,
    pub iso: Option<i64>,
    pub aperture: Option<f64>,
    pub shutter_speed: Option<String>,
    pub focal_length: Option<f64>,
    pub orientation: Option<i64>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

impl Catalog {
    pub async fn open(path: &Path) -> Result<Self, sqlx::Error> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .busy_timeout(Duration::from_secs(5));
        let writer = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options.clone())
            .await?;
        let readers = SqlitePoolOptions::new()
            .max_connections(4)
            .connect_with(options)
            .await?;
        Ok(Self { writer, readers })
    }
    /// Atomic upsert; missing children are retained, matching the legacy repository.
    pub async fn save(&self, photo: &Photo) -> Result<(), sqlx::Error> {
        let mut tx = self.writer.begin().await?;
        sqlx::query("INSERT INTO photos (photo_id, gallery_id, stem_name, perceptual_hash, session_id, burst_id, decision, score, quality_tier, analysis_summary_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(photo_id) DO UPDATE SET gallery_id=excluded.gallery_id, stem_name=excluded.stem_name, perceptual_hash=excluded.perceptual_hash, session_id=excluded.session_id, burst_id=excluded.burst_id, decision=excluded.decision, score=excluded.score, quality_tier=excluded.quality_tier, analysis_summary_json=excluded.analysis_summary_json, created_at=excluded.created_at")
            .bind(&photo.photo_id)
            .bind(&photo.gallery_id)
            .bind(&photo.stem_name)
            .bind(&photo.perceptual_hash)
            .bind(&photo.session_id)
            .bind(&photo.burst_id)
            .bind(&photo.decision)
            .bind(photo.score)
            .bind(&photo.quality_tier)
            .bind(&photo.analysis_summary_json)
            .bind(&photo.created_at)
            .execute(&mut *tx)
            .await?;
        let id: i64 = sqlx::query_scalar("SELECT id FROM photos WHERE photo_id = ?")
            .bind(&photo.photo_id)
            .fetch_one(&mut *tx)
            .await?;
        for file in &photo.files {
            sqlx::query("INSERT INTO files (photo_id, volume_id, import_source_id, last_seen_revision_id, relative_path, source_relative_path, status, role, size_bytes, modified_time, quick_hash, full_hash) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(photo_id, relative_path) DO UPDATE SET volume_id=excluded.volume_id, import_source_id=excluded.import_source_id, last_seen_revision_id=excluded.last_seen_revision_id, source_relative_path=excluded.source_relative_path, status=excluded.status, role=excluded.role, size_bytes=excluded.size_bytes, modified_time=excluded.modified_time, quick_hash=excluded.quick_hash, full_hash=excluded.full_hash")
                .bind(id)
                .bind(file.volume_id)
                .bind(&file.import_source_id)
                .bind(&file.last_seen_revision_id)
                .bind(&file.relative_path)
                .bind(&file.source_relative_path)
                .bind(&file.status)
                .bind(&file.role)
                .bind(file.size_bytes)
                .bind(file.modified_time)
                .bind(&file.quick_hash)
                .bind(&file.full_hash)
                .execute(&mut *tx)
                .await?;
        }
        if let Some(meta) = &photo.metadata {
            sqlx::query("INSERT INTO metadata (photo_id, capture_time, subsecond, camera_make, camera_model, serial_number, lens, iso, aperture, shutter_speed, focal_length, orientation, latitude, longitude) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(photo_id) DO UPDATE SET capture_time=excluded.capture_time, subsecond=excluded.subsecond, camera_make=excluded.camera_make, camera_model=excluded.camera_model, serial_number=excluded.serial_number, lens=excluded.lens, iso=excluded.iso, aperture=excluded.aperture, shutter_speed=excluded.shutter_speed, focal_length=excluded.focal_length, orientation=excluded.orientation, latitude=excluded.latitude, longitude=excluded.longitude")
                .bind(id)
                .bind(&meta.capture_time)
                .bind(&meta.subsecond)
                .bind(&meta.camera_make)
                .bind(&meta.camera_model)
                .bind(&meta.serial_number)
                .bind(&meta.lens)
                .bind(meta.iso)
                .bind(meta.aperture)
                .bind(&meta.shutter_speed)
                .bind(meta.focal_length)
                .bind(meta.orientation)
                .bind(meta.latitude)
                .bind(meta.longitude)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await
    }
    /// Reads all components from one consistent snapshot.
    pub async fn get(&self, photo_id: &str) -> Result<Option<Photo>, sqlx::Error> {
        let mut tx = self.readers.begin().await?;
        let row =
            sqlx::query("SELECT id, photo_id, gallery_id, stem_name, perceptual_hash, session_id, burst_id, decision, score, quality_tier, analysis_summary_json, created_at FROM photos WHERE photo_id = ?")
                .bind(photo_id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let id: i64 = row.try_get("id")?;
        let files = sqlx::query("SELECT volume_id, import_source_id, last_seen_revision_id, relative_path, source_relative_path, status, role, size_bytes, modified_time, quick_hash, full_hash FROM files WHERE photo_id = ? ORDER BY id")
            .bind(id)
            .fetch_all(&mut *tx)
            .await?
            .into_iter()
            .map(|r| Ok(File {
                volume_id: r.try_get("volume_id")?,
                import_source_id: r.try_get("import_source_id")?,
                last_seen_revision_id: r.try_get("last_seen_revision_id")?,
                relative_path: r.try_get("relative_path")?,
                source_relative_path: r.try_get("source_relative_path")?,
                status: r.try_get("status")?,
                role: r.try_get("role")?,
                size_bytes: r.try_get("size_bytes")?,
                modified_time: r.try_get("modified_time")?,
                quick_hash: r.try_get("quick_hash")?,
                full_hash: r.try_get("full_hash")?,
            }))
            .collect::<Result<Vec<_>, sqlx::Error>>()?;
        let metadata =
            sqlx::query("SELECT capture_time, subsecond, camera_make, camera_model, serial_number, lens, iso, aperture, shutter_speed, focal_length, orientation, latitude, longitude FROM metadata WHERE photo_id = ?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .map(|r| {
                    Ok::<Metadata, sqlx::Error>(Metadata {
                        capture_time: r.try_get("capture_time")?,
                        subsecond: r.try_get("subsecond")?,
                        camera_make: r.try_get("camera_make")?,
                        camera_model: r.try_get("camera_model")?,
                        serial_number: r.try_get("serial_number")?,
                        lens: r.try_get("lens")?,
                        iso: r.try_get("iso")?,
                        aperture: r.try_get("aperture")?,
                        shutter_speed: r.try_get("shutter_speed")?,
                        focal_length: r.try_get("focal_length")?,
                        orientation: r.try_get("orientation")?,
                        latitude: r.try_get("latitude")?,
                        longitude: r.try_get("longitude")?,
                    })
                })
                .transpose()?;
        let result = Photo {
            photo_id: row.try_get("photo_id")?,
            gallery_id: row.try_get("gallery_id")?,
            stem_name: row.try_get("stem_name")?,
            perceptual_hash: row.try_get("perceptual_hash")?,
            session_id: row.try_get("session_id")?,
            burst_id: row.try_get("burst_id")?,
            decision: row.try_get("decision")?,
            score: row.try_get("score")?,
            quality_tier: row.try_get("quality_tier")?,
            analysis_summary_json: row.try_get("analysis_summary_json")?,
            created_at: row.try_get("created_at")?,
            files,
            metadata,
        };
        tx.commit().await?;
        Ok(Some(result))
    }
    /// Stop accepting work and wait for all checked-out connections to return.
    pub async fn close(self) {
        self.readers.close().await;
        self.writer.close().await;
    }
}

#[cfg(test)]
mod tests;
