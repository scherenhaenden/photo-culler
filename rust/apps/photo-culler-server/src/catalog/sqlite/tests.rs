use super::*;

async fn setup() -> (tempfile::TempDir, Catalog) {
    let dir = tempfile::tempdir().unwrap();
    let db = Catalog::open(&dir.path().join("catalog.sqlite"))
        .await
        .unwrap();
    for sql in ["CREATE TABLE photos (id INTEGER PRIMARY KEY, photo_id TEXT NOT NULL UNIQUE, stem_name TEXT NOT NULL, decision TEXT, analysis_summary_json TEXT NOT NULL)", "CREATE TABLE files (id INTEGER PRIMARY KEY, photo_id INTEGER NOT NULL REFERENCES photos(id), relative_path TEXT NOT NULL, role TEXT NOT NULL, size_bytes INTEGER NOT NULL CHECK(size_bytes >= 0), modified_time REAL NOT NULL, status TEXT NOT NULL, UNIQUE(photo_id, relative_path))", "CREATE TABLE metadata (id INTEGER PRIMARY KEY, photo_id INTEGER NOT NULL UNIQUE REFERENCES photos(id), camera_make TEXT, camera_model TEXT, iso INTEGER)"] { sqlx::query(sql).execute(&db.writer).await.unwrap(); }
    (dir, db)
}
fn photo() -> Photo {
    Photo {
        photo_id: "p1".into(),
        stem_name: "image".into(),
        decision: "KEEP".into(),
        files: vec![File {
            relative_path: "image.raw".into(),
            role: "RAW".into(),
            size_bytes: 42,
            modified_time: 1.0,
        }],
        metadata: Some(Metadata {
            camera_make: Some("camera".into()),
            camera_model: None,
            iso: Some(400),
        }),
    }
}
#[tokio::test]
async fn roundtrip_and_rollback() {
    let (_dir, db) = setup().await;
    let p = photo();
    db.save(&p).await.unwrap();
    assert_eq!(db.get("p1").await.unwrap(), Some(p.clone()));
    let mut invalid = p.clone();
    invalid.stem_name = "changed".into();
    invalid.files[0].size_bytes = -1;
    assert!(db.save(&invalid).await.is_err());
    assert_eq!(db.get("p1").await.unwrap(), Some(p));
    db.close().await;
}
#[tokio::test]
async fn foreign_keys_are_enabled_on_all_connections() {
    let (_dir, db) = setup().await;
    for pool in [&db.writer, &db.readers] {
        assert!(sqlx::query("INSERT INTO files (photo_id, relative_path, role, size_bytes, modified_time, status) VALUES (999, 'a', 'RAW', 1, 1, 'present')").execute(pool).await.is_err());
    }
    db.close().await;
}
#[tokio::test]
async fn reader_observes_committed_snapshot_while_write_is_open() {
    let (_dir, db) = setup().await;
    db.save(&photo()).await.unwrap();
    let mut tx = db.writer.begin().await.unwrap();
    sqlx::query("UPDATE photos SET decision='REJECT' WHERE photo_id='p1'")
        .execute(&mut *tx)
        .await
        .unwrap();
    let read = tokio::time::timeout(Duration::from_secs(2), db.get("p1"))
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(read.decision, "KEEP");
    tx.commit().await.unwrap();
    assert_eq!(db.get("p1").await.unwrap().unwrap().decision, "REJECT");
    db.close().await;
}
