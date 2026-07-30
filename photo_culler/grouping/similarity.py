"""Deterministic grouping of near-duplicate photos within a shooting window."""

from __future__ import annotations

import hashlib
from datetime import timedelta
from pathlib import Path
from typing import Callable

from photo_culler.core.models import BurstGroup, Photo
from photo_culler.identity.perceptual_hash import compute_dhash, hamming_distance


class SimilarityGrouper:
    """Group visually similar nearby frames using their perceptual dHash."""

    def __init__(self, max_distance: int = 8, max_gap_minutes: float = 10.0):
        self.max_distance = max_distance
        self.max_gap = timedelta(minutes=max_gap_minutes)

    def group(self, photos: list[Photo], resolve_asset: Callable[[Photo], Path | None]) -> tuple[list[BurstGroup], int]:
        """Assign stable group ids and return groups plus photos without a readable asset."""
        skipped = 0
        for photo in photos:
            if photo.perceptual_hash:
                continue
            asset = resolve_asset(photo)
            if asset is None or not asset.exists():
                skipped += 1
                continue
            photo.perceptual_hash = compute_dhash(asset)

        candidates = [photo for photo in photos if photo.perceptual_hash]
        candidates.sort(
            key=lambda photo: (
                photo.metadata.capture_time.isoformat() if photo.metadata and photo.metadata.capture_time else ""
            )
        )
        parent = list(range(len(candidates)))

        def find(index: int) -> int:
            while parent[index] != index:
                parent[index] = parent[parent[index]]
                index = parent[index]
            return index

        def union(left: int, right: int) -> None:
            left_root, right_root = find(left), find(right)
            if left_root != right_root:
                parent[right_root] = left_root

        for index, photo in enumerate(candidates):
            for other_index in range(index - 1, -1, -1):
                other = candidates[other_index]
                if not self._within_time_window(photo, other):
                    break
                if hamming_distance(photo.perceptual_hash, other.perceptual_hash) <= self.max_distance:
                    union(index, other_index)

        clusters: dict[int, list[Photo]] = {}
        for index, photo in enumerate(candidates):
            clusters.setdefault(find(index), []).append(photo)

        groups: list[BurstGroup] = []
        grouped_ids: set[str] = set()
        for members in clusters.values():
            if len(members) < 2:
                continue
            members.sort(key=lambda photo: photo.photo_id)
            digest = hashlib.sha1("|".join(photo.photo_id for photo in members).encode()).hexdigest()[:12]
            group_id = f"similar-{digest}"
            representative = max(members, key=lambda photo: (photo.score, photo.photo_id))
            for photo in members:
                photo.burst_id = group_id
                grouped_ids.add(photo.photo_id)
            groups.append(BurstGroup(group_id, members, representative.photo_id))

        for photo in photos:
            if photo.photo_id not in grouped_ids and (photo.burst_id or "").startswith("similar-"):
                photo.burst_id = None
        return groups, skipped

    def _within_time_window(self, left: Photo, right: Photo) -> bool:
        left_time = left.metadata.capture_time if left.metadata else None
        right_time = right.metadata.capture_time if right.metadata else None
        if left_time is None or right_time is None:
            return left.stem_name[:12] == right.stem_name[:12]
        try:
            return abs(left_time.timestamp() - right_time.timestamp()) <= self.max_gap.total_seconds()
        except OSError, TypeError, ValueError:
            # Mixed/invalid EXIF timezone data must not make grouping fail.
            return left.stem_name[:12] == right.stem_name[:12]
