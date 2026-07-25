"""Basic selection decision rules engine."""

from typing import Dict, List

from ...core.enums import DecisionState, QualityTier
from ...core.models import BurstGroup, Photo


class SelectionRulesEngine:
    """Evaluates photos and assigns culling decision states based on quality and burst similarity."""

    def apply_decisions(self, photos: List[Photo], bursts: List[BurstGroup] = None) -> List[Photo]:
        """Apply basic culling rules across photo set."""
        burst_map: Dict[str, BurstGroup] = {b.burst_id: b for b in (bursts or [])}

        for photo in photos:
            # 1. Hard rejection for corrupted files
            if photo.quality_tier == QualityTier.CORRUPTED:
                photo.decision = DecisionState.REJECT_TECHNICAL
                continue

            # 2. Burst redundancy handling
            if photo.burst_id and photo.burst_id in burst_map:
                bg = burst_map[photo.burst_id]
                if photo.photo_id == bg.representative_photo_id:
                    photo.decision = DecisionState.BEST if photo.score >= 0.60 else DecisionState.KEEP
                else:
                    # Check score delta relative to best photo
                    rep_photo = next((p for p in bg.photos if p.photo_id == bg.representative_photo_id), None)
                    if rep_photo and (rep_photo.score - photo.score) > 0.15:
                        photo.decision = DecisionState.REJECT_REDUNDANT
                    else:
                        photo.decision = DecisionState.ALTERNATE
                continue

            # 3. Non-burst standalone photo decisions
            if photo.score >= 0.75:
                photo.decision = DecisionState.BEST
            elif photo.score >= 0.50:
                photo.decision = DecisionState.KEEP
            elif photo.score >= 0.35:
                photo.decision = DecisionState.REVIEW
            else:
                photo.decision = DecisionState.REJECT_TECHNICAL

        return photos
