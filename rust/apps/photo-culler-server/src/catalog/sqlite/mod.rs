//! SQLite access for the legacy catalog schema, fresh schema initialization, and migrations.
pub mod migrations;

use sqlx::{
    Row, SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
};
use std::{path::Path, time::Duration};

pub mod http_adapter;
pub mod source_bootstrap;

/// One instance per catalog is the service's sole writer. Other processes must not write.
/// Readers use separate connections and see the last committed WAL snapshot.
#[derive(Clone)]
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
    pub decision_revision: u64,
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

// Kept in step with `photo_culler.catalog.schema.Base`. Versioned changes are
// still applied by `migration::migrate` below, after the baseline is installed.
const BASELINE_SCHEMA: &[&str] = &[
    "CREATE TABLE galleries (id VARCHAR(36) PRIMARY KEY, name VARCHAR(255) NOT NULL, created_at DATETIME NOT NULL, updated_at DATETIME NOT NULL)",
    "CREATE TABLE import_sources (id VARCHAR(36) PRIMARY KEY, gallery_id VARCHAR(36) NOT NULL REFERENCES galleries(id), path VARCHAR(2048) NOT NULL, normalized_path VARCHAR(2048) NOT NULL, recursive BOOLEAN NOT NULL DEFAULT 1, created_at DATETIME NOT NULL, UNIQUE(gallery_id, normalized_path))",
    "CREATE TABLE scan_revisions (id VARCHAR(36) PRIMARY KEY, gallery_id VARCHAR(36) NOT NULL REFERENCES galleries(id), source_id VARCHAR(36) NOT NULL REFERENCES import_sources(id), state VARCHAR(32) NOT NULL, discovered INTEGER NOT NULL DEFAULT 0, new_files INTEGER NOT NULL DEFAULT 0, modified_files INTEGER NOT NULL DEFAULT 0, missing_files INTEGER NOT NULL DEFAULT 0, started_at DATETIME NOT NULL, completed_at DATETIME)",
    "CREATE TABLE import_jobs (id VARCHAR(36) PRIMARY KEY, gallery_id VARCHAR(36) NOT NULL REFERENCES galleries(id), source_id VARCHAR(36) NOT NULL REFERENCES import_sources(id), state VARCHAR(32) NOT NULL, discovered INTEGER NOT NULL DEFAULT 0, imported INTEGER NOT NULL DEFAULT 0, issues INTEGER NOT NULL DEFAULT 0, cancel_requested BOOLEAN NOT NULL DEFAULT 0, error TEXT, created_at DATETIME NOT NULL, updated_at DATETIME NOT NULL)",
    "CREATE TABLE volumes (id INTEGER PRIMARY KEY AUTOINCREMENT, volume_id VARCHAR(64) UNIQUE NOT NULL, label VARCHAR(255) NOT NULL, mount_point VARCHAR(1024) NOT NULL, total_bytes INTEGER DEFAULT 0, available_bytes INTEGER DEFAULT 0, is_online BOOLEAN DEFAULT 1)",
    "CREATE TABLE photos (id INTEGER PRIMARY KEY AUTOINCREMENT, gallery_id VARCHAR(36) REFERENCES galleries(id), photo_id VARCHAR(64) UNIQUE NOT NULL, stem_name VARCHAR(255) NOT NULL, perceptual_hash VARCHAR(64), session_id VARCHAR(64), burst_id VARCHAR(64), decision VARCHAR(32) DEFAULT 'UNPROCESSED', score FLOAT DEFAULT 0.0, quality_tier VARCHAR(32) DEFAULT 'fair', created_at DATETIME)",
    "CREATE TABLE photo_decision_revisions (photo_id TEXT PRIMARY KEY REFERENCES photos(photo_id), revision INTEGER NOT NULL CHECK(revision >= 0))",
    "CREATE TABLE files (id INTEGER PRIMARY KEY AUTOINCREMENT, photo_id INTEGER NOT NULL REFERENCES photos(id), volume_id INTEGER REFERENCES volumes(id), relative_path VARCHAR(1024) NOT NULL, role VARCHAR(32) NOT NULL, size_bytes INTEGER NOT NULL CHECK(size_bytes >= 0), modified_time FLOAT NOT NULL, quick_hash VARCHAR(64), full_hash VARCHAR(64), UNIQUE(photo_id, relative_path))",
    "CREATE TABLE metadata (id INTEGER PRIMARY KEY AUTOINCREMENT, photo_id INTEGER NOT NULL UNIQUE REFERENCES photos(id), capture_time DATETIME, subsecond VARCHAR(16), camera_make VARCHAR(128), camera_model VARCHAR(128), serial_number VARCHAR(128), lens VARCHAR(255), iso INTEGER, aperture FLOAT, shutter_speed VARCHAR(32), focal_length FLOAT, orientation INTEGER DEFAULT 1, latitude FLOAT, longitude FLOAT)",
    "CREATE TABLE sessions (id INTEGER PRIMARY KEY AUTOINCREMENT, session_id VARCHAR(64) UNIQUE NOT NULL, name VARCHAR(255) NOT NULL, start_time DATETIME NOT NULL, end_time DATETIME NOT NULL, photo_count INTEGER DEFAULT 0)",
    "CREATE INDEX ix_photos_photo_id ON photos(photo_id)",
    "CREATE INDEX ix_photos_stem_name ON photos(stem_name)",
    "CREATE INDEX ix_photos_perceptual_hash ON photos(perceptual_hash)",
    "CREATE INDEX ix_photos_session_id ON photos(session_id)",
    "CREATE INDEX ix_photos_burst_id ON photos(burst_id)",
    "CREATE INDEX ix_photos_decision ON photos(decision)",
    "CREATE INDEX ix_photos_gallery_id ON photos(gallery_id)",
    "CREATE INDEX ix_import_sources_gallery_id ON import_sources(gallery_id)",
    "CREATE INDEX ix_scan_revisions_gallery_id ON scan_revisions(gallery_id)",
    "CREATE INDEX ix_scan_revisions_source_id ON scan_revisions(source_id)",
    "CREATE INDEX ix_scan_revisions_state ON scan_revisions(state)",
    "CREATE INDEX ix_import_jobs_gallery_id ON import_jobs(gallery_id)",
    "CREATE INDEX ix_import_jobs_source_id ON import_jobs(source_id)",
    "CREATE INDEX ix_import_jobs_state ON import_jobs(state)",
    "CREATE INDEX ix_volumes_volume_id ON volumes(volume_id)",
    "CREATE INDEX ix_files_quick_hash ON files(quick_hash)",
    "CREATE INDEX ix_files_full_hash ON files(full_hash)",
    "CREATE INDEX ix_metadata_capture_time ON metadata(capture_time)",
];

impl Catalog {
    /// Open the application's persistent catalog, creating the baseline schema
    /// for a new file and applying the legacy versioned migrations otherwise.
    /// A file that exists but is not a Photo Culler catalog is rejected rather
    /// than silently initialized over user data.
    pub async fn open_initialized(path: &Path) -> Result<Self, sqlx::Error> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|error| sqlx::Error::Io(error))?;
        }
        let catalog = Self::open(path).await?;
        let photos = sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'photos'",
        )
        .fetch_one(&catalog.writer)
        .await?;
        if photos == 0 {
            let tables: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
            )
            .fetch_one(&catalog.writer)
            .await?;
            if tables != 0 {
                return Err(sqlx::Error::Protocol(
                    "data directory contains a database without the Photo Culler catalog schema"
                        .into(),
                ));
            }
            catalog.create_baseline_schema().await?;
        } else {
            for table in [
                "galleries",
                "files",
                "import_sources",
                "import_jobs",
                "scan_revisions",
                "sessions",
                "volumes",
                "metadata",
            ] {
                let exists: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?",
                )
                .bind(table)
                .fetch_one(&catalog.writer)
                .await?;
                if exists == 0 {
                    return Err(sqlx::Error::Protocol(format!(
                        "catalog is missing required legacy table `{table}`"
                    )));
                }
            }
            catalog.validate_legacy_schema().await?;
        }
        migrations::migrate(&catalog.writer).await?;
        catalog.ensure_decision_revision_table().await?;
        Ok(catalog)
    }

    /// Validate columns shared by all supported Python catalog versions before
    /// applying additive upgrades. Columns introduced by migrations 1–8 are
    /// intentionally excluded so older supported catalogs remain migratable.
    async fn validate_legacy_schema(&self) -> Result<(), sqlx::Error> {
        const REQUIRED: &[(&str, &[&str])] = &[
            ("galleries", &["id", "name", "created_at", "updated_at"]),
            (
                "import_sources",
                &[
                    "id",
                    "gallery_id",
                    "path",
                    "normalized_path",
                    "recursive",
                    "created_at",
                ],
            ),
            (
                "scan_revisions",
                &[
                    "id",
                    "gallery_id",
                    "source_id",
                    "state",
                    "discovered",
                    "new_files",
                    "modified_files",
                    "missing_files",
                    "started_at",
                    "completed_at",
                ],
            ),
            (
                "import_jobs",
                &[
                    "id",
                    "gallery_id",
                    "source_id",
                    "state",
                    "discovered",
                    "imported",
                    "issues",
                    "cancel_requested",
                    "error",
                    "created_at",
                    "updated_at",
                ],
            ),
            (
                "volumes",
                &[
                    "id",
                    "volume_id",
                    "label",
                    "mount_point",
                    "total_bytes",
                    "available_bytes",
                    "is_online",
                ],
            ),
            (
                "photos",
                &[
                    "id",
                    "photo_id",
                    "stem_name",
                    "perceptual_hash",
                    "session_id",
                    "burst_id",
                    "decision",
                    "score",
                    "quality_tier",
                    "created_at",
                ],
            ),
            (
                "files",
                &[
                    "id",
                    "photo_id",
                    "volume_id",
                    "relative_path",
                    "role",
                    "size_bytes",
                    "modified_time",
                    "quick_hash",
                    "full_hash",
                ],
            ),
            (
                "metadata",
                &[
                    "id",
                    "photo_id",
                    "capture_time",
                    "subsecond",
                    "camera_make",
                    "camera_model",
                    "serial_number",
                    "lens",
                    "iso",
                    "aperture",
                    "shutter_speed",
                    "focal_length",
                    "orientation",
                    "latitude",
                    "longitude",
                ],
            ),
            (
                "sessions",
                &[
                    "id",
                    "session_id",
                    "name",
                    "start_time",
                    "end_time",
                    "photo_count",
                ],
            ),
        ];
        for &(table, expected) in REQUIRED {
            let columns: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info(?)")
                .bind(table)
                .fetch_all(&self.writer)
                .await?;
            for &required in expected {
                if !columns.iter().any(|column| column.as_str() == required) {
                    return Err(sqlx::Error::Protocol(
                        format!(
                            "catalog table `{table}` is missing required legacy column `{required}`"
                        )
                        .into(),
                    ));
                }
            }
        }
        Ok(())
    }

    async fn ensure_decision_revision_table(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS photo_decision_revisions (photo_id TEXT PRIMARY KEY REFERENCES photos(photo_id), revision INTEGER NOT NULL CHECK(revision >= 0))",
        )
        .execute(&self.writer)
        .await?;
        Ok(())
    }

    async fn create_baseline_schema(&self) -> Result<(), sqlx::Error> {
        let mut tx = self.writer.begin().await?;
        for statement in BASELINE_SCHEMA {
            sqlx::query(statement).execute(&mut *tx).await?;
        }
        tx.commit().await
    }

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
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS photo_decision_revisions (photo_id TEXT PRIMARY KEY REFERENCES photos(photo_id), revision INTEGER NOT NULL CHECK(revision >= 0))",
        )
        .execute(&mut *tx)
        .await?;
        let previous_decision: Option<Option<String>> =
            sqlx::query_scalar("SELECT decision FROM photos WHERE photo_id = ?")
                .bind(&photo.photo_id)
                .fetch_optional(&mut *tx)
                .await?;
        let decision_changed = previous_decision
            .as_ref()
            .is_some_and(|previous| previous.as_deref() != photo.decision.as_deref());

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

        if decision_changed {
            let previous_revision: Option<i64> = sqlx::query_scalar(
                "SELECT revision FROM photo_decision_revisions WHERE photo_id = ?",
            )
            .bind(&photo.photo_id)
            .fetch_optional(&mut *tx)
            .await?;
            let next_revision = previous_revision
                .unwrap_or(0)
                .checked_add(1)
                .ok_or_else(|| sqlx::Error::Protocol("photo decision revision overflow".into()))?;
            sqlx::query(
                "INSERT INTO photo_decision_revisions(photo_id, revision) VALUES (?, ?) ON CONFLICT(photo_id) DO UPDATE SET revision = excluded.revision",
            )
            .bind(&photo.photo_id)
            .bind(next_revision)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }
    /// Reads all components from one consistent snapshot.
    pub async fn get(&self, photo_id: &str) -> Result<Option<Photo>, sqlx::Error> {
        let mut tx = self.readers.begin().await?;
        let result = read_photo(&mut *tx, photo_id).await?;
        tx.commit().await?;
        Ok(result)
    }

    /// Read a stable, photo-ID-ordered page and all of its child data from one snapshot.
    pub async fn list_gallery(
        &self,
        gallery_id: &str,
        after_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Photo>, sqlx::Error> {
        if !(1..=200).contains(&limit) {
            return Err(sqlx::Error::Protocol(
                "gallery page limit must be between 1 and 200".into(),
            ));
        }

        let mut tx = self.readers.begin().await?;
        let ids = sqlx::query_scalar::<_, String>(
            "SELECT photo_id FROM photos WHERE gallery_id = ? AND (? IS NULL OR photo_id > ?) ORDER BY photo_id ASC LIMIT ?",
        )
        .bind(gallery_id)
        .bind(after_id)
        .bind(after_id)
        .bind(limit as i64)
        .fetch_all(&mut *tx)
        .await?;

        let photos = read_photos(&mut *tx, &ids).await?;
        tx.commit().await?;
        Ok(photos)
    }

    /// Read one stable cursor page with an optional decision filter. The HTTP
    /// layer requests at most 201 records so it can determine whether another
    /// page exists after enforcing its public 200-item limit.
    pub async fn list_gallery_filtered(
        &self,
        gallery_id: &str,
        decision: Option<&str>,
        after_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Photo>, sqlx::Error> {
        if !(1..=201).contains(&limit) {
            return Err(sqlx::Error::Protocol(
                "gallery query limit must be between 1 and 201".into(),
            ));
        }

        let mut tx = self.readers.begin().await?;
        let ids = sqlx::query_scalar::<_, String>(
            "SELECT photo_id FROM photos WHERE gallery_id = ? AND (? IS NULL OR (? = 'keep' AND UPPER(decision) IN ('KEEP','BEST','PROTECTED_BY_COVERAGE')) OR (? = 'alt' AND UPPER(decision) IN ('ALT','ALTERNATE')) OR (? = 'reject' AND UPPER(decision) IN ('REJECT','REJECT_TECHNICAL','REJECT_REDUNDANT')) OR (? = 'undecided' AND UPPER(COALESCE(decision,'')) NOT IN ('KEEP','BEST','PROTECTED_BY_COVERAGE','ALT','ALTERNATE','REJECT','REJECT_TECHNICAL','REJECT_REDUNDANT')) OR (? NOT IN ('keep','alt','reject','undecided') AND UPPER(decision) = UPPER(?))) AND (? IS NULL OR photo_id > ?) ORDER BY photo_id ASC LIMIT ?",
        )
        .bind(gallery_id)
        .bind(decision)
        .bind(decision.map(str::to_ascii_lowercase))
        .bind(decision.map(str::to_ascii_lowercase))
        .bind(decision.map(str::to_ascii_lowercase))
        .bind(decision.map(str::to_ascii_lowercase))
        .bind(decision.map(str::to_ascii_lowercase))
        .bind(decision)
        .bind(after_id)
        .bind(after_id)
        .bind(limit as i64)
        .fetch_all(&mut *tx)
        .await?;

        let photos = read_photos(&mut *tx, &ids).await?;
        tx.commit().await?;
        Ok(photos)
    }

    pub async fn list_galleries(&self) -> Result<Vec<(String, String)>, sqlx::Error> {
        sqlx::query_as::<_, (String, String)>(
            "SELECT id, name FROM galleries ORDER BY created_at, id",
        )
        .fetch_all(&self.readers)
        .await
    }

    pub async fn gallery_for_photo(&self, photo_id: &str) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar("SELECT gallery_id FROM photos WHERE photo_id = ?")
            .bind(photo_id)
            .fetch_optional(&self.readers)
            .await
    }

    /// Resolve candidate media paths in display preference order.
    /// These paths remain server-local; HTTP DTOs never expose them.
    pub async fn display_paths(
        &self,
        photo_id: &str,
    ) -> Result<Vec<std::path::PathBuf>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT f.role, f.status, f.source_relative_path, f.relative_path, s.path, v.mount_point FROM photos p JOIN files f ON f.photo_id = p.id LEFT JOIN import_sources s ON s.id = f.import_source_id LEFT JOIN volumes v ON v.id = f.volume_id WHERE p.photo_id = ? ORDER BY CASE UPPER(f.role) WHEN 'JPEG' THEN 0 WHEN 'IMAGE' THEN 1 WHEN 'RAW' THEN 2 ELSE 3 END, f.id",
        )
        .bind(photo_id)
        .fetch_all(&self.readers)
        .await?;
        use sqlx::Row;
        let mut paths = Vec::new();
        for row in rows {
            let status: String = row.try_get("status")?;
            if !status.eq_ignore_ascii_case("present") {
                continue;
            }
            let relative = row
                .try_get::<Option<String>, _>("source_relative_path")?
                .or(row.try_get::<Option<String>, _>("relative_path")?);
            let Some(relative) = relative else { continue };
            let root = row
                .try_get::<Option<String>, _>("path")?
                .or(row.try_get::<Option<String>, _>("mount_point")?);
            let Some(root) = root else { continue };
            let root = std::path::PathBuf::from(root);
            let relative = std::path::Path::new(&relative);
            if !relative.is_absolute()
                && relative.components().any(|part| {
                    matches!(
                        part,
                        std::path::Component::ParentDir
                            | std::path::Component::RootDir
                            | std::path::Component::Prefix(_)
                    )
                })
            {
                continue;
            }
            let Ok(canonical_root) = std::fs::canonicalize(&root) else {
                continue;
            };
            // Older catalogs can have an absolute relative_path and no
            // source_relative_path. Accept it only when it canonicalizes under
            // the source root recorded alongside that file.
            let candidate = if relative.is_absolute() {
                relative.to_path_buf()
            } else {
                root.join(relative)
            };
            let Ok(canonical_candidate) = std::fs::canonicalize(candidate) else {
                continue;
            };
            if canonical_candidate.starts_with(canonical_root) && canonical_candidate.is_file() {
                paths.push(canonical_candidate);
            }
        }
        Ok(paths)
    }

    /// Return the first candidate for callers that only need one path.
    pub async fn display_path(
        &self,
        photo_id: &str,
    ) -> Result<Option<std::path::PathBuf>, sqlx::Error> {
        Ok(self.display_paths(photo_id).await?.into_iter().next())
    }

    /// Stop accepting work and wait for all checked-out connections to return.
    pub async fn close(self) {
        self.readers.close().await;
        self.writer.close().await;
    }

    /// Clone the serialized writer pool for adjacent persistence adapters.
    /// The catalog and its import service share the same SQLite writer.
    pub fn writer_pool(&self) -> SqlitePool {
        self.writer.clone()
    }
}

/// Load a selected page and all child rows in one query instead of doing
/// several round trips per photo. The caller's ordered IDs define output order.
async fn read_photos(
    connection: &mut sqlx::SqliteConnection,
    ids: &[String],
) -> Result<Vec<Photo>, sqlx::Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let has_decision_revisions: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='photo_decision_revisions')",
    )
    .fetch_one(&mut *connection)
    .await?;

    let mut query =
        sqlx::QueryBuilder::<sqlx::Sqlite>::new("WITH selected(photo_id, position) AS (");
    query.push_values(
        ids.iter().enumerate(),
        |mut values, (position, photo_id)| {
            values
                .push_bind(photo_id)
                .push_bind(i64::try_from(position).unwrap_or(i64::MAX));
        },
    );
    query.push(
        ") SELECT p.id AS photo_row_id, p.photo_id, p.gallery_id, p.stem_name, p.perceptual_hash, p.session_id, p.burst_id, p.decision, ",
    );
    if has_decision_revisions != 0 {
        query.push("COALESCE(dr.revision, 0) AS decision_revision, ");
    } else {
        query.push("0 AS decision_revision, ");
    }
    query.push(
        "p.score, p.quality_tier, p.analysis_summary_json, p.created_at,
         f.id AS file_row_id, f.volume_id, f.import_source_id, f.last_seen_revision_id, f.relative_path, f.source_relative_path, f.status, f.role, f.size_bytes, f.modified_time, f.quick_hash, f.full_hash,
         m.id AS metadata_row_id, m.capture_time, m.subsecond, m.camera_make, m.camera_model, m.serial_number, m.lens, m.iso, m.aperture, m.shutter_speed, m.focal_length, m.orientation, m.latitude, m.longitude
         FROM selected JOIN photos p ON p.photo_id = selected.photo_id ",
    );
    if has_decision_revisions != 0 {
        query.push("LEFT JOIN photo_decision_revisions dr ON dr.photo_id = p.photo_id ");
    }
    query.push(
        "LEFT JOIN files f ON f.photo_id = p.id
         LEFT JOIN metadata m ON m.photo_id = p.id
         ORDER BY selected.position, f.id",
    );
    let rows = query.build().fetch_all(&mut *connection).await?;

    let mut photos = Vec::with_capacity(ids.len());
    let mut last_photo_row_id = None;
    for row in rows {
        let photo_row_id: i64 = row.try_get("photo_row_id")?;
        if last_photo_row_id != Some(photo_row_id) {
            let decision_revision: i64 = row.try_get("decision_revision")?;
            photos.push(Photo {
                photo_id: row.try_get("photo_id")?,
                gallery_id: row.try_get("gallery_id")?,
                stem_name: row.try_get("stem_name")?,
                perceptual_hash: row.try_get("perceptual_hash")?,
                session_id: row.try_get("session_id")?,
                burst_id: row.try_get("burst_id")?,
                decision: row.try_get("decision")?,
                decision_revision: decision_revision.max(0) as u64,
                score: row.try_get("score")?,
                quality_tier: row.try_get("quality_tier")?,
                analysis_summary_json: row.try_get("analysis_summary_json")?,
                created_at: row.try_get("created_at")?,
                files: Vec::new(),
                metadata: None,
            });
            last_photo_row_id = Some(photo_row_id);
            if row.try_get::<Option<i64>, _>("metadata_row_id")?.is_some() {
                let photo = photos.last_mut().ok_or_else(|| {
                    sqlx::Error::Protocol("photo page grouping lost its current row".into())
                })?;
                photo.metadata = Some(Metadata {
                    capture_time: row.try_get("capture_time")?,
                    subsecond: row.try_get("subsecond")?,
                    camera_make: row.try_get("camera_make")?,
                    camera_model: row.try_get("camera_model")?,
                    serial_number: row.try_get("serial_number")?,
                    lens: row.try_get("lens")?,
                    iso: row.try_get("iso")?,
                    aperture: row.try_get("aperture")?,
                    shutter_speed: row.try_get("shutter_speed")?,
                    focal_length: row.try_get("focal_length")?,
                    orientation: row.try_get("orientation")?,
                    latitude: row.try_get("latitude")?,
                    longitude: row.try_get("longitude")?,
                });
            }
        }
        if row.try_get::<Option<i64>, _>("file_row_id")?.is_some() {
            let photo = photos.last_mut().ok_or_else(|| {
                sqlx::Error::Protocol("photo page grouping lost its current row".into())
            })?;
            photo.files.push(File {
                volume_id: row.try_get("volume_id")?,
                import_source_id: row.try_get("import_source_id")?,
                last_seen_revision_id: row.try_get("last_seen_revision_id")?,
                relative_path: row.try_get("relative_path")?,
                source_relative_path: row.try_get("source_relative_path")?,
                status: row.try_get("status")?,
                role: row.try_get("role")?,
                size_bytes: row.try_get("size_bytes")?,
                modified_time: row.try_get("modified_time")?,
                quick_hash: row.try_get("quick_hash")?,
                full_hash: row.try_get("full_hash")?,
            });
        }
    }
    if photos.len() != ids.len() {
        return Err(sqlx::Error::Protocol(
            "photo disappeared inside a read snapshot".into(),
        ));
    }
    Ok(photos)
}

async fn read_photo(
    connection: &mut sqlx::SqliteConnection,
    photo_id: &str,
) -> Result<Option<Photo>, sqlx::Error> {
    let row = sqlx::query("SELECT id, photo_id, gallery_id, stem_name, perceptual_hash, session_id, burst_id, decision, score, quality_tier, analysis_summary_json, created_at FROM photos WHERE photo_id = ?")
        .bind(photo_id)
        .fetch_optional(&mut *connection)
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let id: i64 = row.try_get("id")?;
    let has_decision_revisions: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='photo_decision_revisions')",
    )
    .fetch_one(&mut *connection)
    .await?;
    let decision_revision = if has_decision_revisions != 0 {
        sqlx::query_scalar::<_, i64>(
            "SELECT COALESCE((SELECT revision FROM photo_decision_revisions WHERE photo_id = ?), 0)",
        )
        .bind(row.try_get::<String, _>("photo_id")?)
        .fetch_one(&mut *connection)
        .await?
        .max(0) as u64
    } else {
        0
    };
    let files = sqlx::query("SELECT volume_id, import_source_id, last_seen_revision_id, relative_path, source_relative_path, status, role, size_bytes, modified_time, quick_hash, full_hash FROM files WHERE photo_id = ? ORDER BY id")
        .bind(id)
        .fetch_all(&mut *connection)
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
    let metadata = sqlx::query("SELECT capture_time, subsecond, camera_make, camera_model, serial_number, lens, iso, aperture, shutter_speed, focal_length, orientation, latitude, longitude FROM metadata WHERE photo_id = ?")
        .bind(id)
        .fetch_optional(&mut *connection)
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

    Ok(Some(Photo {
        photo_id: row.try_get("photo_id")?,
        gallery_id: row.try_get("gallery_id")?,
        stem_name: row.try_get("stem_name")?,
        perceptual_hash: row.try_get("perceptual_hash")?,
        session_id: row.try_get("session_id")?,
        burst_id: row.try_get("burst_id")?,
        decision: row.try_get("decision")?,
        decision_revision,
        score: row.try_get("score")?,
        quality_tier: row.try_get("quality_tier")?,
        analysis_summary_json: row.try_get("analysis_summary_json")?,
        created_at: row.try_get("created_at")?,
        files,
        metadata,
    }))
}

#[cfg(test)]
mod tests;
