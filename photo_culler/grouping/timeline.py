"""Timeline shoot session detector."""

from datetime import timedelta
from typing import List, Dict
import uuid

from ..core.models import Photo, SessionRecord


class SessionDetector:
    """Groups photos into shoot sessions based on capture time gaps."""

    def __init__(self, max_gap_minutes: float = 15.0):
        self.max_gap = timedelta(minutes=max_gap_minutes)

    def detect_sessions(self, photos: List[Photo]) -> List[SessionRecord]:
        """Cluster photos into sessions and assign session_id to each Photo object."""
        # Sort photos by capture_time
        sorted_photos = sorted(
            [p for p in photos if p.metadata and p.metadata.capture_time],
            key=lambda x: x.metadata.capture_time
        )
        if not sorted_photos:
            return []

        sessions: List[SessionRecord] = []
        current_photos: List[Photo] = [sorted_photos[0]]

        for p in sorted_photos[1:]:
            prev_time = current_photos[-1].metadata.capture_time
            curr_time = p.metadata.capture_time
            
            if (curr_time - prev_time) <= self.max_gap:
                current_photos.append(p)
            else:
                # Finalize session
                sessions.append(self._create_session(current_photos, len(sessions) + 1))
                current_photos = [p]

        if current_photos:
            sessions.append(self._create_session(current_photos, len(sessions) + 1))

        return sessions

    def _create_session(self, photos: List[Photo], session_index: int) -> SessionRecord:
        sess_id = f"session_{uuid.uuid4().hex[:8]}"
        start_time = photos[0].metadata.capture_time
        end_time = photos[-1].metadata.capture_time
        date_str = start_time.strftime("%Y-%m-%d %H:%M")
        name = f"Session {session_index} ({date_str})"

        for p in photos:
            p.session_id = sess_id

        return SessionRecord(
            session_id=sess_id,
            name=name,
            start_time=start_time,
            end_time=end_time,
            photo_count=len(photos),
        )
