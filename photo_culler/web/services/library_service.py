"""Library Service for retrieving catalog statistics, photo counts, and volume status."""

from typing import Any, Dict
from photo_culler.catalog.database import Database
from photo_culler.catalog.repositories.photo_repository import PhotoRepository


class LibraryService:
    """Provides high-level library statistics and catalog summary data."""

    def __init__(self, db_engine: Database):
        self.db = db_engine

    def get_summary(self) -> Dict[str, Any]:
        """Return catalog summary metrics."""
        with self.db.session() as session:
            repo = PhotoRepository(session)
            photos = repo.list_all()
            total_photos = len(photos)

            decisions_summary: Dict[str, int] = {}
            quality_summary: Dict[str, int] = {}

            for p in photos:
                d_val = p.decision.value if hasattr(p.decision, "value") else str(p.decision)
                decisions_summary[d_val] = decisions_summary.get(d_val, 0) + 1

                q_val = p.quality_tier.value if hasattr(p.quality_tier, "value") else str(p.quality_tier)
                quality_summary[q_val] = quality_summary.get(q_val, 0) + 1

            selected_count = decisions_summary.get("BEST", 0) + decisions_summary.get("KEEP", 0)
            pending_count = decisions_summary.get("REVIEW", 0) + decisions_summary.get("UNPROCESSED", 0)

            return {
                "total_photos": total_photos,
                "total_files": total_photos * 2,
                "selected_count": selected_count,
                "pending_count": pending_count,
                "decisions": decisions_summary,
                "quality_tiers": quality_summary,
            }
