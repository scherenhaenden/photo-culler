"""Decision Service for updating photo culling states non-destructively."""

from typing import Optional
from photo_culler.catalog.database import Database
from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.core.enums import DecisionState
from photo_culler.core.models import Photo


class DecisionService:
    """Updates culling decision states and ratings in SQLite repository."""

    def __init__(self, db_engine: Database):
        self.db = db_engine

    def set_decision(self, photo_id: str, decision: str) -> Optional[Photo]:
        """Update photo decision state non-destructively."""
        with self.db.session() as session:
            repo = PhotoRepository(session)
            photo = repo.get_by_id(photo_id)
            if not photo:
                return None

            state_map = {
                "best": DecisionState.BEST,
                "keep": DecisionState.KEEP,
                "alternate": DecisionState.ALTERNATE,
                "review": DecisionState.REVIEW,
                "reject": DecisionState.REJECT_TECHNICAL,
                "recover": DecisionState.RECOVER,
            }
            new_state = state_map.get(decision.lower(), DecisionState.REVIEW)
            photo.decision = new_state

            repo.save_photo(photo)
            return photo
