"""Fast, local JPEG preselection for burst-heavy shoots.

This module deliberately does *not* claim to recognise a person.  It removes
near-identical frames inside a short shooting window, while leaving a later
photo of the same fan available for the photographer to judge.  That is a
safer first pass than treating every visually similar festival photo as a
duplicate.
"""

from __future__ import annotations

import re
from dataclasses import dataclass
from datetime import datetime
from pathlib import Path

import numpy as np
from PIL import Image, ImageOps, UnidentifiedImageError

from photo_culler.metadata.extractor import MetadataExtractor


@dataclass(frozen=True)
class FrameDescriptor:
    """Small visual signature and quality signals calculated from a JPEG."""

    path: Path
    captured_at: datetime
    sequence_number: int | None
    dhash: np.ndarray
    colour_grid: np.ndarray
    histogram: np.ndarray
    quality: float


@dataclass(frozen=True)
class Preselection:
    """Result for a single source JPEG."""

    frame: FrameDescriptor
    group_id: str
    selected: bool
    representative: Path
    similarity_to_representative: float


class JpegPreselector:
    """Keep the technically strongest distinct frame from each short burst.

    Similarity combines a 256-bit dHash with colour layout and a global colour
    histogram.  dHash alone loses identity-relevant colour and composition
    information, which was the principal source of false grouping here.
    """

    def __init__(
        self,
        *,
        max_gap_seconds: float = 4.0,
        max_sequence_gap: int = 14,
        duplicate_similarity: float = 0.88,
    ):
        if max_gap_seconds <= 0 or max_sequence_gap < 1 or not 0 < duplicate_similarity <= 1:
            raise ValueError("Invalid preselection thresholds")
        self.max_gap_seconds = max_gap_seconds
        self.max_sequence_gap = max_sequence_gap
        self.duplicate_similarity = duplicate_similarity
        self.metadata = MetadataExtractor()

    def describe(self, path: Path) -> FrameDescriptor:
        """Read a bounded preview and derive a stable, inexpensive signature."""
        try:
            with Image.open(path) as source:
                # Request JPEG decoder subsampling *before* an orientation copy.
                # Applying EXIF transpose to a 24 MP frame first can temporarily
                # consume hundreds of MB per photo during a long festival shoot.
                source.draft("RGB", (512, 512))
                source.thumbnail((512, 512), Image.Resampling.LANCZOS)
                image = ImageOps.exif_transpose(source).convert("RGB")
                pixels = np.array(image, dtype=np.float32, copy=True) / 255.0
        except (OSError, UnidentifiedImageError) as exc:
            raise ValueError(f"Cannot read JPEG: {path}") from exc

        luminance = pixels @ np.array([0.299, 0.587, 0.114], dtype=np.float32)
        small = np.asarray(
            Image.fromarray((luminance * 255).astype(np.uint8)).resize((17, 16), Image.Resampling.BILINEAR),
            dtype=np.float32,
        )
        dhash = (small[:, :-1] > small[:, 1:]).reshape(-1)

        # A 4×4 layout distinguishes, for example, a face on the left from a
        # face on the right even when their global histogram is almost equal.
        grid = np.empty((4, 4, 3), dtype=np.float32)
        for row, row_pixels in enumerate(np.array_split(pixels, 4, axis=0)):
            for column, cell in enumerate(np.array_split(row_pixels, 4, axis=1)):
                grid[row, column] = cell.mean(axis=(0, 1))
        colour_grid = grid.reshape(-1)
        histogram = np.concatenate([np.histogram(pixels[:, :, channel], bins=12, range=(0, 1), density=True)[0]
                                    for channel in range(3)]).astype(np.float32)
        histogram /= max(float(histogram.sum()), 1e-6)

        # Strong gradients indicate usable focus/detail; exposure is deliberately
        # broad so bright festival lights do not automatically lose to dull frames.
        gradient_y, gradient_x = np.gradient(luminance)
        detail = float(np.mean(np.hypot(gradient_x, gradient_y)))
        focus = min(detail / 0.14, 1.0)
        mean_light = float(luminance.mean())
        exposure = max(0.0, 1.0 - abs(mean_light - 0.45) / 0.45)
        quality = round(0.78 * focus + 0.22 * exposure, 6)
        record = self.metadata.extract(path)
        return FrameDescriptor(
            path=path,
            captured_at=record.capture_time or datetime.fromtimestamp(path.stat().st_mtime),
            sequence_number=self._sequence_number(path),
            dhash=dhash,
            colour_grid=colour_grid,
            histogram=histogram,
            quality=quality,
        )

    def select(self, paths: list[Path]) -> list[Preselection]:
        """Return every frame with one selected representative per duplicate run."""
        frames = sorted((self.describe(path) for path in paths), key=self._sort_key)
        if not frames:
            return []
        clusters: list[list[FrameDescriptor]] = [[frames[0]]]
        for frame in frames[1:]:
            previous = clusters[-1][-1]
            if self._nearby(previous, frame) and self.similarity(previous, frame) >= self.duplicate_similarity:
                clusters[-1].append(frame)
            else:
                clusters.append([frame])

        results: list[Preselection] = []
        for index, cluster in enumerate(clusters, start=1):
            representative = max(cluster, key=lambda item: (item.quality, item.path.name.lower()))
            group_id = f"burst-{index:04d}"
            for frame in cluster:
                similarity = 1.0 if frame is representative else self.similarity(frame, representative)
                results.append(Preselection(frame, group_id, frame is representative, representative.path, similarity))
        return results

    @staticmethod
    def similarity(left: FrameDescriptor, right: FrameDescriptor) -> float:
        hash_similarity = 1.0 - float(np.mean(left.dhash != right.dhash))
        grid_similarity = 1.0 - min(float(np.mean(np.abs(left.colour_grid - right.colour_grid))) / 0.5, 1.0)
        histogram_similarity = 1.0 - min(float(np.mean(np.abs(left.histogram - right.histogram))) * 12, 1.0)
        return round(0.55 * hash_similarity + 0.30 * grid_similarity + 0.15 * histogram_similarity, 6)

    def _nearby(self, left: FrameDescriptor, right: FrameDescriptor) -> bool:
        time_gap = abs((right.captured_at - left.captured_at).total_seconds())
        if time_gap <= self.max_gap_seconds:
            return True
        if left.sequence_number is None or right.sequence_number is None:
            return False
        return abs(right.sequence_number - left.sequence_number) <= self.max_sequence_gap and time_gap <= 30

    @staticmethod
    def _sequence_number(path: Path) -> int | None:
        match = re.search(r"(\d+)(?!.*\d)", path.stem)
        return int(match.group(1)) if match else None

    @staticmethod
    def _sort_key(frame: FrameDescriptor) -> tuple[datetime, int, str]:
        return (frame.captured_at, frame.sequence_number if frame.sequence_number is not None else -1, frame.path.name.lower())
