//! SQLite access for the legacy catalogue schema. Schema creation/migration is owned by task 18.
use sqlx::{
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous},
    Row, SqlitePool,
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
    pub stem_name: String,
    pub decision: String,
    pub files: Vec<File>,
    pub metadata: Option<Metadata>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct File {
    pub relative_path: String,
    pub role: String,
    pub size_bytes: i64,
    pub modified_time: f64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Metadata {
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub iso: Option<i64>,
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
        sqlx::query("INSERT INTO photos (photo_id, stem_name, decision, analysis_summary_json) VALUES (?, ?, ?, '{}') ON CONFLICT(photo_id) DO UPDATE SET stem_name=excluded.stem_name, decision=excluded.decision")
            .bind(&photo.photo_id).bind(&photo.stem_name).bind(&photo.decision).execute(&mut *tx).await?;
        let id: i64 = sqlx::query_scalar("SELECT id FROM photos WHERE photo_id = ?")
            .bind(&photo.photo_id)
            .fetch_one(&mut *tx)
            .await?;
        for file in &photo.files {
            sqlx::query("INSERT INTO files (photo_id, relative_path, role, size_bytes, modified_time, status) VALUES (?, ?, ?, ?, ?, 'present') ON CONFLICT(photo_id, relative_path) DO UPDATE SET role=excluded.role, size_bytes=excluded.size_bytes, modified_time=excluded.modified_time")
                .bind(id).bind(&file.relative_path).bind(&file.role).bind(file.size_bytes).bind(file.modified_time).execute(&mut *tx).await?;
        }
        if let Some(meta) = &photo.metadata {
            sqlx::query("INSERT INTO metadata (photo_id, camera_make, camera_model, iso) VALUES (?, ?, ?, ?) ON CONFLICT(photo_id) DO UPDATE SET camera_make=excluded.camera_make, camera_model=excluded.camera_model, iso=excluded.iso")
                .bind(id).bind(&meta.camera_make).bind(&meta.camera_model).bind(meta.iso).execute(&mut *tx).await?;
        }
        tx.commit().await
    }
    /// Reads all components from one consistent snapshot.
    pub async fn get(&self, photo_id: &str) -> Result<Option<Photo>, sqlx::Error> {
        let mut tx = self.readers.begin().await?;
        let row =
            sqlx::query("SELECT id, photo_id, stem_name, decision FROM photos WHERE photo_id = ?")
                .bind(photo_id)
                .fetch_optional(&mut *tx)
                .await?;
        let Some(row) = row else {
            tx.commit().await?;
            return Ok(None);
        };
        let id: i64 = row.try_get("id")?;
        let files = sqlx::query("SELECT relative_path, role, size_bytes, modified_time FROM files WHERE photo_id = ? ORDER BY id").bind(id).fetch_all(&mut *tx).await?.into_iter().map(|r| Ok(File { relative_path:r.try_get("relative_path")?, role:r.try_get("role")?, size_bytes:r.try_get("size_bytes")?, modified_time:r.try_get("modified_time")? })).collect::<Result<Vec<_>, sqlx::Error>>()?;
        let metadata =
            sqlx::query("SELECT camera_make, camera_model, iso FROM metadata WHERE photo_id = ?")
                .bind(id)
                .fetch_optional(&mut *tx)
                .await?
                .map(|r| {
                    Ok::<Metadata, sqlx::Error>(Metadata {
                        camera_make: r.try_get("camera_make")?,
                        camera_model: r.try_get("camera_model")?,
                        iso: r.try_get("iso")?,
                    })
                })
                .transpose()?;
        let result = Photo {
            photo_id: row.try_get("photo_id")?,
            stem_name: row.try_get("stem_name")?,
            decision: row.try_get("decision")?,
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
