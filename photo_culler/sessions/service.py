"""Persistent session and burst management built on the timeline detectors."""

from dataclasses import dataclass
from typing import Literal

from sqlalchemy import case, update
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
        self._validate_parameters(profile, timeline_gap_minutes, burst_gap_seconds)

        repository = PhotoRepository(self.session)
        photos = repository.list_all()
        dated = self._dated_photos(photos)
        sessions = self._detect_sessions(profile, dated, timeline_gap_minutes)

        if profile in {"timeline", "hybrid"}:
            self._replace_sessions(sessions)
            self._persist_assignments(photos, "session_id")

        bursts = self._detect_bursts(profile, dated, sessions, burst_gap_seconds)
        self._clear_assignments("burst_id")
        self._persist_assignments(photos, "burst_id")

        return GroupingResult(
            profile=profile,
            sessions=len(sessions) if profile != "burst" else len(self.list_sessions()),
            bursts=len(bursts),
            grouped_photos=sum(1 for photo in photos if photo.session_id or photo.burst_id),
            photos_without_date=len(photos) - len(dated),
        )

    @staticmethod
    def _validate_parameters(profile: str, timeline_gap_minutes: float, burst_gap_seconds: float) -> None:
        if profile not in {"timeline", "burst", "hybrid"}:
            raise ValueError("Unknown grouping profile")
        if profile in {"timeline", "hybrid"} and not 0.1 <= timeline_gap_minutes <= 1440:
            raise ValueError("Timeline gap must be between 0.1 and 1440 minutes")
        if profile in {"burst", "hybrid"} and not 0.05 <= burst_gap_seconds <= 60:
            raise ValueError("Burst gap must be between 0.05 and 60 seconds")

    @staticmethod
    def _dated_photos(photos):
        return [photo for photo in photos if photo.metadata and photo.metadata.capture_time]

    def _detect_sessions(self, profile: GroupingProfile, dated, timeline_gap_minutes: float):
        if profile not in {"timeline", "hybrid"}:
            return []
        return SessionDetector(timeline_gap_minutes).detect_sessions(dated)

    def _replace_sessions(self, sessions) -> None:
        self.session.query(SessionDB).delete(synchronize_session=False)
        self._clear_assignments("session_id")
        self.session.add_all(
            [
                SessionDB(
                    session_id=record.session_id,
                    name=record.name,
                    start_time=record.start_time,
                    end_time=record.end_time,
                    photo_count=record.photo_count,
                )
                for record in sessions
            ]
        )

    def _detect_bursts(self, profile: GroupingProfile, dated, sessions, burst_gap_seconds: float):
        if profile not in {"burst", "hybrid"}:
            return []
        detector = BurstDetector(burst_gap_seconds)
        if profile == "burst":
            return detector.detect_bursts(dated)
        bursts = []
        for record in sessions:
            members = [photo for photo in dated if photo.session_id == record.session_id]
            bursts.extend(detector.detect_bursts(members))
        return bursts

    def _clear_assignments(self, column: str) -> None:
        self.session.execute(update(PhotoDB).values({column: None}))

    def _persist_assignments(self, photos, column: str) -> None:
        assignments = {photo.photo_id: getattr(photo, column) for photo in photos if getattr(photo, column)}
        if assignments:
            self.session.execute(
                update(PhotoDB)
                .where(PhotoDB.photo_id.in_(assignments))
                .values({column: case(assignments, value=PhotoDB.photo_id)})
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
