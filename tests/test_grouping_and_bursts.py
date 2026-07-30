"""Unit tests for grouping and burst detection."""

from datetime import datetime, timedelta

from photo_culler.bursts.temporal_bursts import BurstDetector
from photo_culler.core.models import MetadataRecord, Photo
from photo_culler.grouping.similarity import SimilarityGrouper
from photo_culler.grouping.timeline import SessionDetector


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
