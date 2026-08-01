"""Library Service for retrieving catalog statistics, photo counts, and volume status."""

from typing import Any, Dict

from sqlalchemy import func

from photo_culler.catalog.database import Database
from photo_culler.catalog.schema import FileDB, PhotoDB


class LibraryService:
    """Provides high-level library statistics and catalog summary data."""

    def __init__(self, db_engine: Database):
        self.db = db_engine

    def get_summary(self) -> Dict[str, Any]:
        """Return catalog summary metrics."""
        with self.db.session() as session:
            total_photos = session.query(func.count(PhotoDB.id)).scalar() or 0
            total_files = session.query(func.count(FileDB.id)).scalar() or 0
            decisions_summary = dict(session.query(PhotoDB.decision, func.count(PhotoDB.id)).group_by(PhotoDB.decision))
            quality_summary = dict(
                session.query(PhotoDB.quality_tier, func.count(PhotoDB.id)).group_by(PhotoDB.quality_tier)
            )

            selected_count = decisions_summary.get("BEST", 0) + decisions_summary.get("KEEP", 0)
            pending_count = decisions_summary.get("REVIEW", 0) + decisions_summary.get("UNPROCESSED", 0)

            return {
                "total_photos": total_photos,
                "total_files": total_files,
                "selected_count": selected_count,
                "pending_count": pending_count,
                "decisions": decisions_summary,
                "quality_tiers": quality_summary,
            }
