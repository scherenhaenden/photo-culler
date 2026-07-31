"""Persistent session and burst management built on the timeline detectors."""

from dataclasses import dataclass
from typing import Literal

from sqlalchemy.orm import Session

from photo_culler.bursts.temporal_bursts import BurstDetector
from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.catalog.schema import PhotoDB, SessionDB
from photo_culler.grouping.timeline import SessionDetector

GroupingProfile = Literal["timeline", "burst", "hybrid"]


@dataclass(frozen=True)
class GroupingResult:
    profile: GroupingProfile
    sessions: int
    bursts: int
    grouped_photos: int
    photos_without_date: int


class SessionManagementService:
    """Apply grouping profiles and keep their assignments consistent in one transaction."""

    def __init__(self, session: Session):
        self.session = session

    def list_sessions(self) -> list[SessionDB]:
        return self.session.query(SessionDB).order_by(SessionDB.start_time.asc()).all()

    def apply_profile(
        self,
        profile: GroupingProfile,
        *,
        timeline_gap_minutes: float = 15.0,
        burst_gap_seconds: float = 1.5,
    ) -> GroupingResult:
        if profile not in {"timeline", "burst", "hybrid"}:
            raise ValueError("Unknown grouping profile")
        if not 0.1 <= timeline_gap_minutes <= 1440:
            raise ValueError("Timeline gap must be between 0.1 and 1440 minutes")
        if not 0.05 <= burst_gap_seconds <= 60:
            raise ValueError("Burst gap must be between 0.05 and 60 seconds")

        repository = PhotoRepository(self.session)
        photos = repository.list_all()
        dated = [photo for photo in photos if photo.metadata and photo.metadata.capture_time]
        sessions = []

        if profile in {"timeline", "hybrid"}:
            self.session.query(SessionDB).delete(synchronize_session=False)
            for photo in photos:
                photo.session_id = None
            sessions = SessionDetector(timeline_gap_minutes).detect_sessions(dated)
            for record in sessions:
                self.session.add(
                    SessionDB(
                        session_id=record.session_id,
                        name=record.name,
                        start_time=record.start_time,
                        end_time=record.end_time,
                        photo_count=record.photo_count,
                    )
                )

        for photo in photos:
            photo.burst_id = None

        bursts = []
        if profile in {"burst", "hybrid"}:
            detector = BurstDetector(burst_gap_seconds)
            if profile == "hybrid":
                # A burst can never bridge the inactivity boundary of a shoot session.
                for record in sessions:
                    members = [photo for photo in dated if photo.session_id == record.session_id]
                    bursts.extend(detector.detect_bursts(members))
            else:
                bursts = detector.detect_bursts(dated)

        for photo in photos:
            repository.save_photo(photo)

        return GroupingResult(
            profile=profile,
            sessions=len(sessions) if profile != "burst" else len(self.list_sessions()),
            bursts=len(bursts),
            grouped_photos=sum(1 for photo in photos if photo.session_id or photo.burst_id),
            photos_without_date=len(photos) - len(dated),
        )

    def rename(self, session_id: str, name: str) -> SessionDB:
        clean_name = name.strip()
        if not clean_name or len(clean_name) > 255:
            raise ValueError("Session name must contain between 1 and 255 characters")
        record = self.session.query(SessionDB).filter_by(session_id=session_id).first()
        if record is None:
            raise LookupError("Session not found")
        record.name = clean_name
        return record

    def delete(self, session_id: str) -> None:
        record = self.session.query(SessionDB).filter_by(session_id=session_id).first()
        if record is None:
            raise LookupError("Session not found")
        self.session.query(PhotoDB).filter_by(session_id=session_id).update({PhotoDB.session_id: None})
        self.session.delete(record)
