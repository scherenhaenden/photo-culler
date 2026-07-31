"""Unit tests for grouping and burst detection."""

from datetime import datetime, timedelta

import pytest

from photo_culler.bursts.temporal_bursts import BurstDetector
from photo_culler.catalog.database import Database
from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.catalog.schema import PhotoDB, SessionDB
from photo_culler.core.models import MetadataRecord, Photo
from photo_culler.grouping.similarity import SimilarityGrouper
from photo_culler.grouping.timeline import SessionDetector
from photo_culler.sessions import SessionManagementService


def test_session_detector():
    base_time = datetime(2026, 7, 25, 18, 0, 0)
    photos = [
        Photo("p1", "DSC_01", metadata=MetadataRecord(capture_time=base_time)),
        Photo("p2", "DSC_02", metadata=MetadataRecord(capture_time=base_time + timedelta(minutes=5))),
        Photo("p3", "DSC_03", metadata=MetadataRecord(capture_time=base_time + timedelta(minutes=30))),
    ]

    detector = SessionDetector(max_gap_minutes=15.0)
    sessions = detector.detect_sessions(photos)
    assert len(sessions) == 2
    assert photos[0].session_id == photos[1].session_id
    assert photos[2].session_id != photos[0].session_id


def test_burst_detector():
    base_time = datetime(2026, 7, 25, 18, 0, 0)
    photos = [
        Photo("p1", "DSC_01", metadata=MetadataRecord(capture_time=base_time), score=0.7),
        Photo("p2", "DSC_02", metadata=MetadataRecord(capture_time=base_time + timedelta(seconds=0.5)), score=0.9),
        Photo("p3", "DSC_03", metadata=MetadataRecord(capture_time=base_time + timedelta(seconds=1.0)), score=0.6),
        Photo("p4", "DSC_04", metadata=MetadataRecord(capture_time=base_time + timedelta(seconds=10.0)), score=0.8),
    ]

    detector = BurstDetector(max_burst_gap_seconds=1.5)
    bursts = detector.detect_bursts(photos)
    assert len(bursts) == 1
    burst = bursts[0]
    assert len(burst.photos) == 3
    assert burst.representative_photo_id == "p2"


def test_hybrid_session_management_persists_sessions_and_scoped_bursts(tmp_path):
    database = Database(tmp_path / "sessions.db")
    base_time = datetime(2026, 7, 25, 18, 0, 0)
    photos = [
        Photo("p1", "P1", metadata=MetadataRecord(capture_time=base_time), score=0.4),
        Photo("p2", "P2", metadata=MetadataRecord(capture_time=base_time + timedelta(seconds=1)), score=0.9),
        Photo("p3", "P3", metadata=MetadataRecord(capture_time=base_time + timedelta(minutes=30)), score=0.7),
        Photo("p4", "P4", metadata=MetadataRecord(capture_time=base_time + timedelta(minutes=30, seconds=1))),
        Photo("undated", "Undated"),
    ]
    with database.session() as session:
        repository = PhotoRepository(session)
        for photo in photos:
            repository.save_photo(photo)

    with database.session() as session:
        result = SessionManagementService(session).apply_profile(
            "hybrid", timeline_gap_minutes=15, burst_gap_seconds=1.5
        )
        assert (result.sessions, result.bursts, result.grouped_photos, result.photos_without_date) == (2, 2, 4, 1)

    with database.session() as session:
        stored_sessions = session.query(SessionDB).order_by(SessionDB.start_time).all()
        stored_photos = session.query(PhotoDB).order_by(PhotoDB.photo_id).all()
        assert len(stored_sessions) == 2
        assert len({photo.session_id for photo in stored_photos if photo.session_id}) == 2
        assert len({photo.burst_id for photo in stored_photos if photo.burst_id}) == 2
        burst_ids_by_session = {
            session_id: {photo.burst_id for photo in stored_photos if photo.session_id == session_id and photo.burst_id}
            for session_id in {photo.session_id for photo in stored_photos if photo.session_id}
        }
        assert not set.intersection(*burst_ids_by_session.values())
        service = SessionManagementService(session)
        with pytest.raises(ValueError):
            service.rename(stored_sessions[0].session_id, "   ")
        with pytest.raises(ValueError):
            service.rename(stored_sessions[0].session_id, "x" * 256)
        with pytest.raises(LookupError):
            service.rename("missing", "No session")
        with pytest.raises(LookupError):
            service.delete("missing")
        service.rename(stored_sessions[0].session_id, "Ceremonia")
        removed_id = stored_sessions[1].session_id
        service.delete(removed_id)

    with database.session() as session:
        assert session.query(SessionDB).count() == 1
        assert session.query(SessionDB).one().name == "Ceremonia"
        assert session.query(PhotoDB).filter_by(session_id=removed_id).count() == 0


def test_similarity_grouper_assigns_representative_for_nearby_visual_matches(tmp_path):
    from PIL import Image

    base_time = datetime(2026, 7, 25, 18, 0, 0)
    first = tmp_path / "first.jpg"
    second = tmp_path / "second.jpg"
    distant = tmp_path / "distant.jpg"
    Image.new("RGB", (80, 60), (100, 80, 60)).save(first)
    Image.new("RGB", (80, 60), (100, 80, 60)).save(second)
    Image.new("RGB", (80, 60), (10, 200, 20)).save(distant)
    photos = [
        Photo("first", "first", metadata=MetadataRecord(capture_time=base_time), score=0.6),
        Photo("second", "second", metadata=MetadataRecord(capture_time=base_time + timedelta(seconds=2)), score=0.9),
        Photo("distant", "distant", metadata=MetadataRecord(capture_time=base_time + timedelta(minutes=30)), score=0.8),
    ]
    assets = {"first": first, "second": second, "distant": distant}

    groups, skipped = SimilarityGrouper().group(photos, lambda photo: assets[photo.photo_id])

    assert skipped == 0
    assert len(groups) == 1
    assert groups[0].representative_photo_id == "second"
    assert photos[0].burst_id == photos[1].burst_id
    assert photos[2].burst_id is None
