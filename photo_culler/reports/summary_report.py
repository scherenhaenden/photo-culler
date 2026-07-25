"""Summary and culling report generator."""

from typing import List, Dict, Any
from ..core.models import Photo, SessionRecord, BurstGroup
from ..core.enums import DecisionState, QualityTier


class ReportGenerator:
    """Generates structured summaries and statistics for processed photo sets."""

    def generate_summary(
        self,
        photos: List[Photo],
        sessions: List[SessionRecord] = None,
        bursts: List[BurstGroup] = None
    ) -> Dict[str, Any]:
        total_photos = len(photos)
        
        # Decision breakdown
        decisions: Dict[str, int] = {d.value: 0 for d in DecisionState}
        for p in photos:
            decisions[p.decision.value] += 1

        # Quality tier breakdown
        quality_tiers: Dict[str, int] = {q.value: 0 for q in QualityTier}
        for p in photos:
            quality_tiers[p.quality_tier.value] += 1

        # File role counts
        raw_count = sum(1 for p in photos if any(f.role.value == "raw" for f in p.files))
        jpeg_count = sum(1 for p in photos if any(f.role.value == "jpeg" for f in p.files))

        kept_count = decisions["KEEP"] + decisions["BEST"] + decisions["ALTERNATE"] + decisions["PROTECTED_BY_COVERAGE"]
        rejected_count = decisions["REJECT_TECHNICAL"] + decisions["REJECT_REDUNDANT"]

        return {
            "total_photos": total_photos,
            "total_subfiles": sum(len(p.files) for p in photos),
            "raw_count": raw_count,
            "jpeg_count": jpeg_count,
            "sessions_detected": len(sessions or []),
            "burst_groups_detected": len(bursts or []),
            "culling_summary": {
                "total_kept": kept_count,
                "total_rejected": rejected_count,
                "keep_rate_pct": round((kept_count / total_photos * 100), 2) if total_photos > 0 else 0.0,
            },
            "decisions": decisions,
            "quality_tiers": quality_tiers,
        }
