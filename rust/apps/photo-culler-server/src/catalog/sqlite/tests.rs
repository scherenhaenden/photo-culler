use super::*;

async fn setup() -> (tempfile::TempDir, Catalog) {
    let dir = tempfile::tempdir().unwrap();
    let db = Catalog::open(&dir.path().join("catalog.sqlite"))
        .await
        .unwrap();
    for sql in [
        "CREATE TABLE galleries (id TEXT PRIMARY KEY)",
        "CREATE TABLE volumes (id INTEGER PRIMARY KEY)",
        "CREATE TABLE import_sources (id TEXT PRIMARY KEY)",
        "CREATE TABLE scan_revisions (id TEXT PRIMARY KEY)",
        "CREATE TABLE photos (id INTEGER PRIMARY KEY, photo_id TEXT NOT NULL UNIQUE, gallery_id TEXT REFERENCES galleries(id), stem_name TEXT NOT NULL, perceptual_hash TEXT, session_id TEXT, burst_id TEXT, decision TEXT, score REAL, quality_tier TEXT, analysis_summary_json TEXT NOT NULL, created_at TEXT)",
        "CREATE TABLE files (id INTEGER PRIMARY KEY, photo_id INTEGER NOT NULL REFERENCES photos(id), volume_id INTEGER REFERENCES volumes(id), import_source_id TEXT REFERENCES import_sources(id), last_seen_revision_id TEXT REFERENCES scan_revisions(id), relative_path TEXT NOT NULL, source_relative_path TEXT, status TEXT NOT NULL, role TEXT NOT NULL, size_bytes INTEGER NOT NULL CHECK(size_bytes >= 0), modified_time REAL NOT NULL, quick_hash TEXT, full_hash TEXT, UNIQUE(photo_id, relative_path))",
        "CREATE TABLE metadata (id INTEGER PRIMARY KEY, photo_id INTEGER NOT NULL UNIQUE REFERENCES photos(id), capture_time TEXT, subsecond TEXT, camera_make TEXT, camera_model TEXT, serial_number TEXT, lens TEXT, iso INTEGER, aperture REAL, shutter_speed TEXT, focal_length REAL, orientation INTEGER, latitude REAL, longitude REAL)",
    ] {
        sqlx::query(sql).execute(&db.writer).await.unwrap();
    }
    for sql in [
        "INSERT INTO galleries(id) VALUES ('gallery-1'), ('gallery-2')",
        "INSERT INTO volumes(id) VALUES (17), (18)",
        "INSERT INTO import_sources(id) VALUES ('source-1')",
        "INSERT INTO scan_revisions(id) VALUES ('revision-1'), ('revision-2')",
    ] {
        sqlx::query(sql).execute(&db.writer).await.unwrap();
    }
    (dir, db)
}
fn photo() -> Photo {
    Photo {
        photo_id: "p1".into(),
        gallery_id: Some("gallery-1".into()),
        stem_name: "image".into(),
        perceptual_hash: Some("perceptual-hash".into()),
        session_id: Some("session-1".into()),
        burst_id: Some("burst-1".into()),
        decision: Some("KEEP".into()),
        score: Some(0.875),
        quality_tier: Some("good".into()),
        analysis_summary_json: r#"{"profile_id":"profile-1"}"#.into(),
        created_at: Some("2026-11-18 10:11:12.000000".into()),
        files: vec![File {
            volume_id: Some(17),
            import_source_id: Some("source-1".into()),
            last_seen_revision_id: Some("revision-1".into()),
            relative_path: "image.raw".into(),
            source_relative_path: Some("DCIM/100/image.raw".into()),
            status: "present".into(),
            role: "RAW".into(),
            size_bytes: 42,
            modified_time: 1.0,
            quick_hash: Some("quick-hash".into()),
            full_hash: Some("full-hash".into()),
        }],
        metadata: Some(Metadata {
            capture_time: Some("2026-11-18 10:11:12.123456".into()),
            subsecond: Some("123456".into()),
            camera_make: Some("camera".into()),
            camera_model: Some("camera-model".into()),
            serial_number: Some("serial-1".into()),
            lens: Some("lens-1".into()),
            iso: Some(400),
            aperture: Some(2.8),
            shutter_speed: Some("1/250".into()),
            focal_length: Some(50.0),
            orientation: Some(6),
            latitude: Some(48.1372),
            longitude: Some(11.5756),
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
async fn upsert_updates_all_legacy_fields() {
    let (_dir, db) = setup().await;
    db.save(&photo()).await.unwrap();

    let mut updated = photo();
    updated.gallery_id = Some("gallery-2".into());
    updated.stem_name = "renamed".into();
    updated.perceptual_hash = Some("perceptual-hash-2".into());
    updated.session_id = None;
    updated.burst_id = Some("burst-2".into());
    updated.decision = Some("REJECT".into());
    updated.score = Some(0.125);
    updated.quality_tier = Some("poor".into());
    updated.analysis_summary_json = r#"{"profile_id":"profile-2"}"#.into();
    updated.created_at = Some("2026-11-19 11:12:13.000000".into());
    updated.files[0].volume_id = Some(18);
    updated.files[0].import_source_id = None;
    updated.files[0].last_seen_revision_id = Some("revision-2".into());
    updated.files[0].relative_path = "image.raw".into();
    updated.files[0].source_relative_path = None;
    updated.files[0].status = "missing".into();
    updated.files[0].role = "JPEG".into();
    updated.files[0].size_bytes = 84;
    updated.files[0].modified_time = 2.0;
    updated.files[0].quick_hash = None;
    updated.files[0].full_hash = Some("full-hash-2".into());
    let metadata = updated.metadata.as_mut().unwrap();
    metadata.capture_time = None;
    metadata.subsecond = None;
    metadata.camera_make = Some("camera-2".into());
    metadata.camera_model = None;
    metadata.serial_number = None;
    metadata.lens = Some("lens-2".into());
    metadata.iso = Some(800);
    metadata.aperture = Some(4.0);
    metadata.shutter_speed = Some("1/500".into());
    metadata.focal_length = Some(85.0);
    metadata.orientation = Some(1);
    metadata.latitude = None;
    metadata.longitude = Some(-1.2345);

    db.save(&updated).await.unwrap();
    assert_eq!(db.get("p1").await.unwrap(), Some(updated));
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
    assert_eq!(read.decision.as_deref(), Some("KEEP"));
    tx.commit().await.unwrap();
    assert_eq!(
        db.get("p1").await.unwrap().unwrap().decision.as_deref(),
        Some("REJECT")
    );
    db.close().await;
}

#[tokio::test]
async fn gallery_pages_are_ordered_filtered_and_keep_child_data() {
    let (_dir, db) = setup().await;
    for id in ["p3", "p1", "p2"] {
        let mut record = photo();
        record.photo_id = id.into();
        db.save(&record).await.unwrap();
    }
    let mut other_gallery = photo();
    other_gallery.photo_id = "p0".into();
    other_gallery.gallery_id = Some("gallery-2".into());
    db.save(&other_gallery).await.unwrap();

    let first = db.list_gallery("gallery-1", None, 2).await.unwrap();
    assert_eq!(
        first
            .iter()
            .map(|item| item.photo_id.as_str())
            .collect::<Vec<_>>(),
        ["p1", "p2"]
    );
    assert_eq!(first[0].files, photo().files);
    assert_eq!(first[0].metadata, photo().metadata);

    let second = db.list_gallery("gallery-1", Some("p2"), 2).await.unwrap();
    assert_eq!(
        second
            .iter()
            .map(|item| item.photo_id.as_str())
            .collect::<Vec<_>>(),
        ["p3"]
    );
    assert!(db.list_gallery("gallery-1", None, 0).await.is_err());
    assert!(db.list_gallery("gallery-1", None, 201).await.is_err());
    db.close().await;
}
