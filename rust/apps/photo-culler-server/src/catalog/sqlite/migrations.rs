//! SQLx connection adapter for the existing legacy catalog migrations.
//!
//! A single checked-out connection is kept for the entire migration so that
//! `BEGIN IMMEDIATE`, every schema change, version records, and `COMMIT` share
//! one SQLite transaction.

use sqlx::{Row, Sqlite, SqlitePool, pool::PoolConnection};

struct SqlxConnection<'a>(&'a mut PoolConnection<Sqlite>);

impl super::super::migration::Connection for SqlxConnection<'_> {
    type Error = sqlx::Error;

    fn execute<'a>(
        &'a mut self,
        sql: &'a str,
    ) -> super::super::migration::MigrationFuture<'a, Result<(), Self::Error>> {
        Box::pin(async move {
            sqlx::query(sql).execute(&mut **self.0).await?;
            Ok(())
        })
    }

    fn max_applied_version(
        &mut self,
    ) -> super::super::migration::MigrationFuture<'_, Result<Option<i64>, Self::Error>> {
        Box::pin(async move {
            sqlx::query_scalar::<_, Option<i64>>("SELECT MAX(version) FROM schema_migrations")
                .fetch_one(&mut **self.0)
                .await
        })
    }

    fn unsupported_version(&self, version: i64) -> Self::Error {
        sqlx::Error::Protocol(format!("unsupported schema migration version {version}").into())
    }

    fn discard(&mut self) {
        self.0.close_on_drop();
    }

    fn has_column<'a>(
        &'a mut self,
        table: &'a str,
        column: &'a str,
    ) -> super::super::migration::MigrationFuture<'a, Result<bool, Self::Error>> {
        Box::pin(async move {
            let exists: i64 = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM pragma_table_info(?) WHERE name = ?)",
            )
            .bind(table)
            .bind(column)
            .fetch_one(&mut **self.0)
            .await?;
            Ok(exists != 0)
        })
    }

    fn has_single_column_unique_index<'a>(
        &'a mut self,
        table: &'a str,
        column: &'a str,
    ) -> super::super::migration::MigrationFuture<'a, Result<bool, Self::Error>> {
        Box::pin(async move {
            let indexes = sqlx::query("SELECT name, \"unique\", partial FROM pragma_index_list(?)")
                .bind(table)
                .fetch_all(&mut **self.0)
                .await?;
            for index in indexes {
                let unique: i64 = index.try_get("unique")?;
                let partial: i64 = index.try_get("partial")?;
                if unique != 1 || partial != 0 {
                    continue;
                }
                let name: String = index.try_get("name")?;
                let columns = sqlx::query("SELECT name FROM pragma_index_info(?)")
                    .bind(name)
                    .fetch_all(&mut **self.0)
                    .await?;
                if columns.len() == 1 && columns[0].try_get::<String, _>("name")? == column {
                    return Ok(true);
                }
            }
            Ok(false)
        })
    }

    fn validate_photo_ids(
        &mut self,
    ) -> super::super::migration::MigrationFuture<'_, Result<(), Self::Error>> {
        Box::pin(async move {
            let invalid: i64 = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM photos WHERE photo_id IS NULL OR trim(photo_id) = '')",
            )
            .fetch_one(&mut **self.0)
            .await?;
            if invalid != 0 {
                return Err(sqlx::Error::Protocol(
                    "legacy catalog contains a null or empty photo_id".into(),
                ));
            }
            Ok(())
        })
    }

    fn applied(
        &mut self,
        version: i32,
    ) -> super::super::migration::MigrationFuture<'_, Result<bool, Self::Error>> {
        Box::pin(async move {
            let exists: i64 = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?)",
            )
            .bind(version)
            .fetch_one(&mut **self.0)
            .await?;
            Ok(exists != 0)
        })
    }
}

/// Apply every pending legacy-catalog migration atomically.
pub async fn migrate(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    let mut connection = pool.acquire().await?;
    super::super::migration::migrate(&mut SqlxConnection(&mut connection)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn legacy_catalog() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        for statement in [
            "PRAGMA foreign_keys = ON",
            "CREATE TABLE photos (id INTEGER PRIMARY KEY, photo_id TEXT UNIQUE NOT NULL, decision TEXT, session_id TEXT)",
            "CREATE TABLE files (id INTEGER PRIMARY KEY, photo_id INTEGER NOT NULL REFERENCES photos(id), role TEXT)",
            "CREATE TABLE import_sources (id TEXT PRIMARY KEY)",
            "CREATE TABLE import_jobs (id TEXT PRIMARY KEY)",
            "CREATE TABLE scan_revisions (id TEXT PRIMARY KEY)",
            "CREATE TABLE profiles (id TEXT PRIMARY KEY, revision INTEGER)",
            "CREATE TABLE sessions (id INTEGER PRIMARY KEY, session_id TEXT UNIQUE)",
            "INSERT INTO sessions VALUES (1, 'session-1')",
            "INSERT INTO photos VALUES (7, 'photo-7', 'KEEP', 'session-1')",
            "INSERT INTO files VALUES (12, 7, 'JPEG')",
        ] {
            sqlx::query(statement).execute(&pool).await.unwrap();
        }
        pool
    }

    #[tokio::test]
    async fn upgrades_legacy_catalog_idempotently_without_changing_rows() {
        let pool = legacy_catalog().await;

        migrate(&pool).await.unwrap();
        migrate(&pool).await.unwrap();

        let migrations: i64 = sqlx::query_scalar("SELECT count(*) FROM schema_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
        let preserved: (String, String, String) = sqlx::query_as(
            "SELECT p.photo_id, p.decision, s.session_id FROM photos p JOIN sessions s ON s.session_id = p.session_id WHERE p.id = 7",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let files: i64 = sqlx::query_scalar("SELECT count(*) FROM files WHERE photo_id = 7")
            .fetch_one(&pool)
            .await
            .unwrap();

        assert_eq!(migrations, 9);
        assert_eq!(
            preserved,
            ("photo-7".into(), "KEEP".into(), "session-1".into())
        );
        assert_eq!(files, 1);
    }

    #[tokio::test]
    async fn failed_migration_rolls_back_schema_and_version_rows() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query("CREATE TABLE photos (id INTEGER PRIMARY KEY)")
            .execute(&pool)
            .await
            .unwrap();

        assert!(migrate(&pool).await.is_err());

        let schema_migrations: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let added_column: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('photos') WHERE name = 'gallery_id')",
        )
        .fetch_one(&pool)
        .await
        .unwrap();

        assert_eq!(schema_migrations, 0);
        assert_eq!(added_column, 0);
    }
}
