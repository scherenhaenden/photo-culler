"""Temporal burst sequence detector."""

from datetime import timedelta
from typing import List
import uuid

from ..core.models import Photo, BurstGroup


class BurstDetector:
    """Identifies high-speed photo bursts (sub-second or short interval gaps)."""

    def __init__(self, max_burst_gap_seconds: float = 1.5):
        self.max_gap = timedelta(seconds=max_burst_gap_seconds)

    def detect_bursts(self, photos: List[Photo]) -> List[BurstGroup]:
        """Detect burst groups and assign burst_id to each Photo object."""
        sorted_photos = sorted(
            [p for p in photos if p.metadata and p.metadata.capture_time],
            key=lambda x: x.metadata.capture_time
        )
        if not sorted_photos:
            return []

        bursts: List[BurstGroup] = []
        current_burst: List[Photo] = [sorted_photos[0]]

        for p in sorted_photos[1:]:
            prev_time = current_burst[-1].metadata.capture_time
            curr_time = p.metadata.capture_time

            if (curr_time - prev_time) <= self.max_gap:
                current_burst.append(p)
            else:
                if len(current_burst) >= 2:
                    bursts.append(self._create_burst(current_burst))
                current_burst = [p]

        if len(current_burst) >= 2:
            bursts.append(self._create_burst(current_burst))

        return bursts

    def _create_burst(self, photos: List[Photo]) -> BurstGroup:
        burst_id = f"burst_{uuid.uuid4().hex[:8]}"
        for p in photos:
            p.burst_id = burst_id

        # Pick best scoring photo as representative if scored, else first
        best_photo = max(photos, key=lambda x: x.score)

        return BurstGroup(
            burst_id=burst_id,
            photos=photos,
            representative_photo_id=best_photo.photo_id,
        )
