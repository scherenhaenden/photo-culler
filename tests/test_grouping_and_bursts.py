"""Unit tests for grouping and burst detection."""

import pytest
from datetime import datetime, timedelta

from photo_culler.core.models import Photo, MetadataRecord
from photo_culler.grouping.timeline import SessionDetector
from photo_culler.bursts.temporal_bursts import BurstDetector


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
