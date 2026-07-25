"""PhotoSelector query resolver."""

from datetime import datetime
from pathlib import Path
from typing import List, Optional

from ...catalog.repositories.photo_repository import PhotoRepository
from ...core.enums import DecisionState
from ...core.models import Photo


class PhotoSelector:
    """Central query resolver for filtering photos across catalog records."""

    def __init__(self, repository: PhotoRepository):
        self.repo = repository

    def resolve(
        self,
        path: Optional[Path] = None,
        session: Optional[str] = None,
        volume: Optional[str] = None,
        time_from: Optional[datetime] = None,
        time_to: Optional[datetime] = None,
        status: Optional[str] = None,
        decision: Optional[str] = None,
        photo_id: Optional[str] = None,
        hash_val: Optional[str] = None,
    ) -> List[Photo]:
        """Query repository and return matching Photo domain objects."""
        all_photos = self.repo.list_all()
        results: List[Photo] = []

        for p in all_photos:
            # 1. photo_id filter
            if photo_id and p.photo_id != photo_id:
                continue

            # 2. hash filter
            if hash_val and (not p.perceptual_hash or hash_val.lower() not in p.perceptual_hash.lower()):
                continue

            # 3. session filter
            if session and (not p.session_id or session.lower() not in p.session_id.lower()):
                continue

            # 4. decision filter
            if decision and p.decision.value.lower() != decision.lower():
                continue

            # 5. status filter (e.g. pending = UNPROCESSED)
            if status:
                if status.lower() == "pending" and p.decision != DecisionState.UNPROCESSED:
                    continue
                elif status.lower() == "processed" and p.decision == DecisionState.UNPROCESSED:
                    continue

            # 6. path filter
            if path:
                path_resolved = Path(path).resolve()
                match_path = False
                for f in p.files:
                    if str(path_resolved) in str(f.path.resolve()):
                        match_path = True
                        break
                if not match_path:
                    continue

            # 7. time_from & time_to filter
            if p.metadata and p.metadata.capture_time:
                ct = p.metadata.capture_time
                if time_from and ct < time_from:
                    continue
                if time_to and ct > time_to:
                    continue

            results.append(p)

        return results
