"""Unit tests for catalog database and PhotoRepository."""

from datetime import datetime
from pathlib import Path

import pytest

from photo_culler.catalog.database import CatalogBackend, CatalogConfig, Database
from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.core.enums import DecisionState, FileRole, QualityTier
from photo_culler.core.models import FileRecord, MetadataRecord, Photo


@pytest.fixture
def memory_db():
    db = Database(":memory:")
    return db


def test_photo_repository(memory_db):
    with memory_db.session() as session:
        repo = PhotoRepository(session)

        file_raw = FileRecord(
            path=Path("/photos/DSC_0001.NEF"),
            role=FileRole.RAW,
            size_bytes=25000000,
            modified_time=1700000000.0,
        )
        file_jpeg = FileRecord(
            path=Path("/photos/DSC_0001.JPG"),
            role=FileRole.JPEG,
            size_bytes=5000000,
            modified_time=1700000000.0,
        )
        meta = MetadataRecord(
            capture_time=datetime(2026, 7, 25, 18, 30, 0),
            camera_make="Nikon",
            camera_model="Z6",
            iso=400,
            aperture=2.8,
            shutter_speed="1/500",
        )

        photo = Photo(
            photo_id="photo_001",
            stem_name="DSC_0001",
            files=[file_raw, file_jpeg],
            metadata=meta,
            decision=DecisionState.KEEP,
            score=0.88,
            quality_tier=QualityTier.EXCELLENT,
        )

        repo.save_photo(photo)

    # Verify retrieval
    with memory_db.session() as session:
        repo = PhotoRepository(session)
        assert repo.count() == 1
        retrieved = repo.get_by_id("photo_001")
        assert retrieved is not None
        assert retrieved.stem_name == "DSC_0001"
        assert len(retrieved.files) == 2
        assert retrieved.metadata.camera_model == "Z6"
        assert retrieved.decision == DecisionState.KEEP
        assert retrieved.score == 0.88


def test_catalog_config_supports_sqlite_urls_and_rejects_unknown_backends(tmp_path):
    config = CatalogConfig.resolve(db_url=f"sqlite:///{tmp_path / 'catalog.db'}")
    assert config.backend is CatalogBackend.SQLITE

    db = Database(db_url=config.url)
    assert db.config == config

    with pytest.raises(ValueError, match="Unsupported catalog backend"):
        CatalogConfig.resolve(db_url="mysql://localhost/photo_culler")


def test_save_photo_refreshes_existing_file_metadata(memory_db):
    path = Path("/photos/DSC_0001.JPG")
    with memory_db.session() as session:
        repository = PhotoRepository(session)
        repository.save_photo(
            Photo(
                photo_id="photo_001",
                stem_name="DSC_0001",
                files=[FileRecord(path, FileRole.JPEG, 100, 1_700_000_000.0)],
            )
        )
        repository.save_photo(
            Photo(
                photo_id="photo_001",
                stem_name="DSC_0001",
                files=[FileRecord(path, FileRole.JPEG, 200, 1_700_000_001.0)],
            )
        )

    with memory_db.session() as session:
        file = PhotoRepository(session).get_by_id("photo_001").files[0]
        assert (file.size_bytes, file.modified_time) == (200, 1_700_000_001.0)
