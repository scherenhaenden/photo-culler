"""Deterministic grouping of near-duplicate photos within a shooting window."""

from __future__ import annotations

import hashlib
from datetime import timedelta
from pathlib import Path
from typing import Callable

from photo_culler.core.models import BurstGroup, Photo
from photo_culler.identity.perceptual_hash import compute_dhash, hamming_distance, is_valid_perceptual_hash


class SimilarityGrouper:
    """Group visually similar nearby frames using their perceptual dHash."""

    def __init__(self, max_distance: int = 8, max_gap_minutes: float = 10.0):
        self.max_distance = max_distance
        self.max_gap = timedelta(minutes=max_gap_minutes)

    def group(
        self,
        photos: list[Photo],
        resolve_asset: Callable[[Photo], Path | None],
        on_progress: Callable[[int, int, str], None] | None = None,
    ) -> tuple[list[BurstGroup], int]:
        """Assign stable group ids and return groups plus photos without a readable asset."""
        skipped = 0
        total_steps = max(1, len(photos) * 2)
        completed_steps = 0
        for photo in photos:
            if not photo.perceptual_hash:
                asset = resolve_asset(photo)
                if asset is None or not asset.exists():
                    skipped += 1
                else:
                    photo.perceptual_hash = compute_dhash(asset)
            completed_steps += 1
            if on_progress:
                on_progress(completed_steps, total_steps, photo.stem_name)

        candidates = [photo for photo in photos if is_valid_perceptual_hash(photo.perceptual_hash)]
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

        # A distance of <= 8 guarantees that two 64-bit hashes share at least
        # one exact chunk when split into nine chunks. The index avoids an
        # O(n²) scan of a large catalog while retaining every valid candidate.
        chunk_index: dict[tuple[int, str], list[int]] = {}
        exact_representatives: dict[tuple[str, str], int] = {}
        for index, photo in enumerate(candidates):
            photo_hash = photo.perceptual_hash or ""
            time_key = "" if photo.metadata and photo.metadata.capture_time else photo.stem_name[:12]
            exact_key = (photo_hash, time_key)
            exact_index = exact_representatives.get(exact_key)
            if exact_index is not None and self._within_time_window(photo, candidates[exact_index]):
                union(index, exact_index)

            possible_matches: set[int] = set()
            for chunk_position, chunk in enumerate(self._hash_chunks(photo_hash)):
                possible_matches.update(chunk_index.get((chunk_position, chunk), []))
            for other_index in possible_matches:
                other = candidates[other_index]
                if other.perceptual_hash == photo_hash or not self._within_time_window(photo, other):
                    continue
                if hamming_distance(photo_hash, other.perceptual_hash or "") <= self.max_distance:
                    union(index, other_index)
            for chunk_position, chunk in enumerate(self._hash_chunks(photo_hash)):
                chunk_index.setdefault((chunk_position, chunk), []).append(index)
            exact_representatives[exact_key] = index
            completed_steps += 1
            if on_progress:
                on_progress(completed_steps, total_steps, photo.stem_name)

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

    def _hash_chunks(self, perceptual_hash: str) -> list[str]:
        """Split a hexadecimal hash so a close hash shares one exact chunk."""
        try:
            bits = f"{int(perceptual_hash, 16):0{len(perceptual_hash) * 4}b}"
        except ValueError:
            bits = perceptual_hash
        chunk_count = self.max_distance + 1
        base_size, remainder = divmod(len(bits), chunk_count)
        chunks = []
        cursor = 0
        for index in range(chunk_count):
            size = base_size + (1 if index < remainder else 0)
            chunks.append(bits[cursor : cursor + size])
            cursor += size
        return chunks
