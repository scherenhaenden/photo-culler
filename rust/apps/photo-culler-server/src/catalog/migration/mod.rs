//! Python catalog upgrades 1–7 plus Rust additive upgrades 8–9. Run against a
//! consistent, writable catalog copy.
//! Integration: implement `Connection` using the catalog SQLite connection, then
//! call `migrate` before opening repositories. The connection must own its write
//! transaction and report SQLite errors without swallowing them.

use std::{future::Future, pin::Pin};

pub type MigrationFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub trait Connection: Send {
    type Error;
    fn execute<'a>(&'a mut self, sql: &'a str) -> MigrationFuture<'a, Result<(), Self::Error>>;
    /// Return the highest recorded migration version, or `None` when the
    /// migration ledger is empty. Implementations must fail on malformed ledgers.
    fn max_applied_version(&mut self) -> MigrationFuture<'_, Result<Option<i64>, Self::Error>>;
    /// Construct the implementation's native error for a schema version this
    /// binary does not understand.
    fn unsupported_version(&self, version: i64) -> Self::Error;
    /// Discard the underlying connection if manual transaction state is uncertain.
    fn discard(&mut self) {}
    fn has_column<'a>(
        &'a mut self,
        table: &'a str,
        column: &'a str,
    ) -> MigrationFuture<'a, Result<bool, Self::Error>>;
    /// Return whether the table has a non-partial, single-column unique index
    /// on `column`. Composite and partial indexes do not satisfy identity
    /// uniqueness for all rows.
    fn has_single_column_unique_index<'a>(
        &'a mut self,
        table: &'a str,
        column: &'a str,
    ) -> MigrationFuture<'a, Result<bool, Self::Error>>;
    /// Fail when existing photo rows have no usable stable identity.
    fn validate_photo_ids(&mut self) -> MigrationFuture<'_, Result<(), Self::Error>>;
    fn applied(&mut self, version: i32) -> MigrationFuture<'_, Result<bool, Self::Error>>;
}

async fn column<C: Connection>(
    db: &mut C,
    table: &str,
    name: &str,
    definition: &str,
) -> Result<(), C::Error> {
    if !db.has_column(table, name).await? {
        db.execute(&format!(
            "ALTER TABLE {table} ADD COLUMN {name} {definition}"
        ))
        .await?;
    }
    Ok(())
}

async fn upgrade<C: Connection>(db: &mut C, v: i32) -> Result<(), C::Error> {
    match v {
        1 => {
            column(db, "photos", "gallery_id", "VARCHAR(36)").await?;
            db.execute("CREATE INDEX IF NOT EXISTS ix_photos_gallery_id ON photos (gallery_id)")
                .await?;
        }
        2 => {
            column(
                db,
                "import_jobs",
                "pause_requested",
                "BOOLEAN NOT NULL DEFAULT 0",
            )
            .await?;
            column(db, "import_jobs", "resume_state", "VARCHAR(32)").await?;
        }
        3 => {
            for (table, name, def) in [
                (
                    "import_sources",
                    "status",
                    "VARCHAR(32) NOT NULL DEFAULT 'online'",
                ),
                ("import_sources", "last_seen_at", "DATETIME"),
                ("import_jobs", "scan_revision_id", "VARCHAR(36)"),
                ("files", "import_source_id", "VARCHAR(36)"),
                ("files", "last_seen_revision_id", "VARCHAR(36)"),
                ("files", "source_relative_path", "VARCHAR(1024)"),
                ("files", "status", "VARCHAR(32) NOT NULL DEFAULT 'present'"),
            ] {
                column(db, table, name, def).await?;
            }
            for (table, name) in [
                ("import_sources", "status"),
                ("import_jobs", "scan_revision_id"),
                ("files", "import_source_id"),
                ("files", "last_seen_revision_id"),
                ("files", "status"),
            ] {
                db.execute(&format!(
                    "CREATE INDEX IF NOT EXISTS ix_{table}_{name} ON {table} ({name})"
                ))
                .await?;
            }
        }
        4 => {
            column(
                db,
                "import_sources",
                "exclude_patterns",
                "TEXT NOT NULL DEFAULT '[]'",
            )
            .await?
        }
        5 => {
            column(
                db,
                "scan_revisions",
                "moved_files",
                "INTEGER NOT NULL DEFAULT 0",
            )
            .await?
        }
        6 => {
            db.execute("CREATE TABLE IF NOT EXISTS edit_documents (id VARCHAR(36) PRIMARY KEY, photo_id INTEGER NOT NULL UNIQUE REFERENCES photos(id), contract_version INTEGER NOT NULL DEFAULT 1, revision INTEGER NOT NULL DEFAULT 0, recipe_json TEXT NOT NULL DEFAULT '{}', undo_stack_json TEXT NOT NULL DEFAULT '[]', redo_stack_json TEXT NOT NULL DEFAULT '[]', created_at DATETIME NOT NULL, updated_at DATETIME NOT NULL)").await?;
            db.execute("CREATE UNIQUE INDEX IF NOT EXISTS ix_edit_documents_photo_id ON edit_documents (photo_id)").await?;
        }
        7 => {
            column(
                db,
                "photos",
                "analysis_summary_json",
                "TEXT NOT NULL DEFAULT '{}'",
            )
            .await?
        }
        8 => {
            // Import reconciliation and per-observation classification probe
            // the exact source/path pair. Keep these lookups indexed as the
            // catalog grows instead of rescanning every file on each photo.
            db.execute("CREATE INDEX IF NOT EXISTS ix_files_import_source_path ON files (import_source_id, source_relative_path)")
                .await?;
        }
        9 => {
            // Stable photo IDs are catalog identities, not just a lookup hint.
            // Legacy catalogs may lack the unique constraint in their table
            // definition; add an equivalent index transactionally. Existing
            // duplicate IDs make this fail and roll back rather than allowing
            // ambiguous photo updates.
            db.validate_photo_ids().await?;
            if !db
                .has_single_column_unique_index("photos", "photo_id")
                .await?
            {
                db.execute("CREATE UNIQUE INDEX ux_photos_photo_id ON photos (photo_id)")
                    .await?;
            }
        }
        _ => unreachable!(),
    }
    Ok(())
}

/// All pending upgrades and version records commit atomically. Existing rows are
/// never copied or rewritten. Caller must ensure no transaction is already open.
pub async fn migrate<C: Connection>(db: &mut C) -> Result<(), C::Error> {
    db.execute("BEGIN IMMEDIATE").await?;
    let result = async {
        db.execute("CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at VARCHAR(40) NOT NULL)").await?;
        if let Some(version) = db.max_applied_version().await?
            && !(0..=9).contains(&version)
        {
            return Err(db.unsupported_version(version));
        }
        for v in 1..=9 {
            if !db.applied(v).await? {
                upgrade(db, v).await?;
                db.execute(&format!("INSERT INTO schema_migrations(version, applied_at) VALUES ({v}, CURRENT_TIMESTAMP)")).await?;
            }
        }
        db.execute("COMMIT").await
    }.await;
    if result.is_err() {
        if db.execute("ROLLBACK").await.is_err() {
            db.discard();
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::{CStr, CString, c_char, c_int, c_void};
    use std::ptr;
    #[link(name = "sqlite3")]
    unsafe extern "C" {
        fn sqlite3_open(name: *const c_char, db: *mut *mut c_void) -> c_int;
        fn sqlite3_close(db: *mut c_void) -> c_int;
        fn sqlite3_exec(
            db: *mut c_void,
            sql: *const c_char,
            cb: Option<
                unsafe extern "C" fn(
                    *mut c_void,
                    c_int,
                    *mut *mut c_char,
                    *mut *mut c_char,
                ) -> c_int,
            >,
            arg: *mut c_void,
            err: *mut *mut c_char,
        ) -> c_int;
        fn sqlite3_free(p: *mut c_void);
    }
    struct Sqlite(*mut c_void);
    // This test-only adapter owns one SQLite connection and its async methods
    // perform only synchronous SQLite calls. Moving the unique owner between
    // executor threads is safe; it is never shared concurrently.
    unsafe impl Send for Sqlite {}

    impl Sqlite {
        fn new() -> Self {
            let mut db = ptr::null_mut();
            unsafe { assert_eq!(sqlite3_open(c":memory:".as_ptr(), &mut db), 0) };
            Self(db)
        }
        fn query(&mut self, sql: &str) -> Result<Vec<Vec<String>>, String> {
            unsafe extern "C" fn collect(
                arg: *mut c_void,
                n: c_int,
                values: *mut *mut c_char,
                _: *mut *mut c_char,
            ) -> c_int {
                unsafe {
                    let rows = &mut *(arg as *mut Vec<Vec<String>>);
                    rows.push(
                        (0..n as usize)
                            .map(|i| {
                                let p = *values.add(i);
                                if p.is_null() {
                                    "<null>".into()
                                } else {
                                    CStr::from_ptr(p).to_string_lossy().into_owned()
                                }
                            })
                            .collect(),
                    );
                }
                0
            }
            let mut rows = Vec::new();
            let mut error = ptr::null_mut();
            let rc = unsafe {
                sqlite3_exec(
                    self.0,
                    CString::new(sql).unwrap().as_ptr(),
                    Some(collect),
                    &mut rows as *mut _ as *mut c_void,
                    &mut error,
                )
            };
            if rc == 0 {
                Ok(rows)
            } else {
                let msg = unsafe { CStr::from_ptr(error).to_string_lossy().into_owned() };
                unsafe { sqlite3_free(error as *mut c_void) };
                Err(msg)
            }
        }
    }
    impl Drop for Sqlite {
        fn drop(&mut self) {
            unsafe {
                sqlite3_close(self.0);
            }
        }
    }
    impl Connection for Sqlite {
        type Error = String;
        fn execute<'a>(&'a mut self, sql: &'a str) -> MigrationFuture<'a, Result<(), String>> {
            Box::pin(async move { self.query(sql).map(|_| ()) })
        }
        fn max_applied_version(&mut self) -> MigrationFuture<'_, Result<Option<i64>, String>> {
            Box::pin(async move {
                let rows = self.query("SELECT MAX(version) FROM schema_migrations")?;
                let value = rows.first().and_then(|row| row.first()).map(String::as_str);
                match value {
                    None | Some("<null>") => Ok(None),
                    Some(value) => value.parse::<i64>().map(Some).map_err(|e| e.to_string()),
                }
            })
        }
        fn unsupported_version(&self, version: i64) -> String {
            format!("unsupported schema migration version {version}")
        }
        fn has_column<'a>(
            &'a mut self,
            table: &'a str,
            column: &'a str,
        ) -> MigrationFuture<'a, Result<bool, String>> {
            Box::pin(async move {
                Ok(self
                    .query(&format!("PRAGMA table_info({table})"))?
                    .iter()
                    .any(|r| r[1] == column))
            })
        }
        fn has_single_column_unique_index<'a>(
            &'a mut self,
            table: &'a str,
            column: &'a str,
        ) -> MigrationFuture<'a, Result<bool, String>> {
            Box::pin(async move {
                let indexes = self.query(&format!("PRAGMA index_list({table})"))?;
                for index in indexes {
                    // PRAGMA index_list columns: seq, name, unique, origin,
                    // partial. Partial indexes do not cover every photo row.
                    if index.get(2).map(String::as_str) != Some("1")
                        || index.get(4).map(String::as_str) == Some("1")
                    {
                        continue;
                    }
                    let Some(name) = index.get(1) else {
                        continue;
                    };
                    let escaped_name = name.replace('\'', "''");
                    let columns = self.query(&format!("PRAGMA index_info('{escaped_name}')"))?;
                    if columns.len() == 1 && columns[0].get(2).map(String::as_str) == Some(column) {
                        return Ok(true);
                    }
                }
                Ok(false)
            })
        }
        fn validate_photo_ids(&mut self) -> MigrationFuture<'_, Result<(), String>> {
            Box::pin(async move {
                let rows = self.query(
                    "SELECT EXISTS(SELECT 1 FROM photos WHERE photo_id IS NULL OR trim(photo_id) = '')",
                )?;
                if rows.first().and_then(|row| row.first()).map(String::as_str) == Some("1") {
                    Err("catalog contains a null or empty photo_id".into())
                } else {
                    Ok(())
                }
            })
        }
        fn applied(&mut self, v: i32) -> MigrationFuture<'_, Result<bool, String>> {
            Box::pin(async move {
                Ok(!self
                    .query(&format!(
                        "SELECT version FROM schema_migrations WHERE version={v}"
                    ))?
                    .is_empty())
            })
        }
    }
    #[tokio::test]
    async fn historical_catalog_twice_preserves_manifest() {
        let mut db = Sqlite::new();
        db.query("PRAGMA foreign_keys=ON;
          CREATE TABLE photos(id INTEGER PRIMARY KEY, photo_id TEXT UNIQUE, decision TEXT, session_id TEXT);
          CREATE TABLE sessions(id INTEGER PRIMARY KEY, session_id TEXT UNIQUE);
          CREATE TABLE files(id INTEGER PRIMARY KEY, photo_id INTEGER REFERENCES photos(id), role TEXT);
          CREATE TABLE import_sources(id TEXT PRIMARY KEY); CREATE TABLE import_jobs(id TEXT PRIMARY KEY); CREATE TABLE scan_revisions(id TEXT PRIMARY KEY);
          CREATE TABLE profiles(id TEXT PRIMARY KEY, revision INTEGER);
          CREATE TABLE edit_documents(id TEXT PRIMARY KEY, photo_id INTEGER UNIQUE REFERENCES photos(id), contract_version INTEGER, revision INTEGER, recipe_json TEXT, undo_stack_json TEXT, redo_stack_json TEXT, created_at TEXT, updated_at TEXT);
          INSERT INTO sessions VALUES(4,'s1'); INSERT INTO photos VALUES(12,'photo-a','KEEP','s1');
          INSERT INTO files VALUES(21,12,'RAW'),(22,12,'JPEG'); INSERT INTO profiles VALUES('profile-a',3);
          INSERT INTO edit_documents VALUES('edit-a',12,1,8,'{\"profile_id\":\"profile-a\",\"profile_revision\":3}','[]','[]','t','t');").unwrap();
        let manifest = "SELECT p.id,p.photo_id,p.decision,p.session_id,s.id,f.id,f.role,e.id,e.contract_version,e.revision,e.recipe_json,pr.id,pr.revision FROM photos p JOIN sessions s ON s.session_id=p.session_id JOIN files f ON f.photo_id=p.id JOIN edit_documents e ON e.photo_id=p.id JOIN profiles pr ON pr.id='profile-a' ORDER BY f.id";
        let before = db.query(manifest).unwrap();
        for _ in 0..2 {
            migrate(&mut db).await.unwrap();
            assert_eq!(db.query(manifest).unwrap(), before);
            assert_eq!(
                db.query("SELECT count(*) FROM schema_migrations").unwrap()[0][0],
                "9"
            );
            assert!(db.query("PRAGMA foreign_key_check").unwrap().is_empty());
        }
    }
    #[tokio::test]
    async fn failed_upgrade_rolls_back_schema_and_versions() {
        let mut db = Sqlite::new();
        db.query("CREATE TABLE photos(id INTEGER PRIMARY KEY)")
            .unwrap();
        assert!(migrate(&mut db).await.is_err());
        assert!(!db.has_column("photos", "gallery_id").await.unwrap());
        assert!(db.query("SELECT * FROM schema_migrations").is_err());
    }
}
