"""Persistent non-destructive edit recipes and deterministic preview rendering."""

import io
import json
import threading
import uuid
from collections import OrderedDict
from pathlib import Path

import numpy as np
from PIL import Image, ImageOps, UnidentifiedImageError
from sqlalchemy import select

from photo_culler.catalog.database import Database
from photo_culler.catalog.schema import EditDocumentDB, FileDB, PhotoDB, utc_now

DEFAULT_RECIPE: dict[str, float | int] = {
    "exposure": 0.0,
    "temperature": 6500,
    "tint": 0.0,
}


class EditService:
    """Store edit parameters as recipes and render disposable JPEG previews."""

    def __init__(self, database: Database, cache_entries: int = 64) -> None:
        self.database = database
        self.cache_entries = max(1, cache_entries)
        self._cache: OrderedDict[tuple[str, int, int], bytes] = OrderedDict()
        self._cache_lock = threading.Lock()

    def get_document(self, photo_id: str) -> dict[str, object]:
        """Return or create the versioned edit document for one logical photo."""
        with self.database.session() as session:
            photo = session.execute(select(PhotoDB).where(PhotoDB.photo_id == photo_id)).scalar_one_or_none()
            if photo is None:
                raise LookupError("Photo not found")
            document = session.execute(
                select(EditDocumentDB).where(EditDocumentDB.photo_id == photo.id)
            ).scalar_one_or_none()
            if document is None:
                document = EditDocumentDB(
                    id=str(uuid.uuid4()),
                    photo_id=photo.id,
                    recipe_json=json.dumps(DEFAULT_RECIPE, sort_keys=True),
                )
                session.add(document)
                session.flush()
            return self._to_dto(photo_id, document)

    def update_document(
        self,
        photo_id: str,
        *,
        exposure: float | None = None,
        temperature: int | None = None,
        tint: float | None = None,
    ) -> dict[str, object]:
        """Persist validated global edits and append the prior recipe to history."""
        changes: dict[str, float | int] = {}
        if exposure is not None:
            if not -5.0 <= exposure <= 5.0:
                raise ValueError("Exposure must be between -5 and 5 EV")
            changes["exposure"] = round(float(exposure), 4)
        if temperature is not None:
            if not 2000 <= temperature <= 12000:
                raise ValueError("Temperature must be between 2000K and 12000K")
            changes["temperature"] = int(temperature)
        if tint is not None:
            if not -100.0 <= tint <= 100.0:
                raise ValueError("Tint must be between -100 and 100")
            changes["tint"] = round(float(tint), 4)
        if not changes:
            return self.get_document(photo_id)

        with self.database.session() as session:
            photo, document = self._get_rows(session, photo_id)
            current = self._recipe(document)
            updated = {**current, **changes}
            if updated != current:
                undo_stack = self._stack(document.undo_stack_json)
                undo_stack.append(current)
                document.undo_stack_json = json.dumps(undo_stack[-50:], sort_keys=True)
                document.redo_stack_json = "[]"
                document.recipe_json = json.dumps(updated, sort_keys=True)
                document.revision += 1
                document.updated_at = utc_now()
            dto = self._to_dto(photo.photo_id, document)
        self._invalidate(photo_id)
        return dto

    def undo(self, photo_id: str) -> dict[str, object]:
        """Restore the previous recipe while preserving a redo entry."""
        return self._move_history(photo_id, undo=True)

    def redo(self, photo_id: str) -> dict[str, object]:
        """Reapply the most recently undone recipe."""
        return self._move_history(photo_id, undo=False)

    def render_preview(self, photo_id: str, max_size: int = 1600) -> bytes:
        """Render a disposable JPEG preview without modifying the source file."""
        if not 64 <= max_size <= 4096:
            raise ValueError("Preview size must be between 64 and 4096 pixels")
        document = self.get_document(photo_id)
        revision = document["revision"]
        if not isinstance(revision, int):
            raise ValueError("Invalid edit document revision")
        cache_key = (photo_id, revision, max_size)
        with self._cache_lock:
            cached = self._cache.get(cache_key)
            if cached is not None:
                self._cache.move_to_end(cache_key)
                return cached

        source = self._resolve_source(photo_id)
        recipe = document["recipe"]
        assert isinstance(recipe, dict)
        try:
            with Image.open(source) as opened:
                image = ImageOps.exif_transpose(opened).convert("RGB")
                image.thumbnail((max_size, max_size), Image.Resampling.LANCZOS)
                pixels = np.asarray(image, dtype=np.float32).copy()
        except (OSError, UnidentifiedImageError) as exc:
            raise ValueError("The selected source cannot be rendered") from exc

        exposure_gain = 2.0 ** float(recipe["exposure"])
        temperature_offset = (float(recipe["temperature"]) - 6500.0) / 5500.0
        tint_offset = float(recipe["tint"]) / 100.0
        gains = np.array(
            [
                1.0 + (0.28 * temperature_offset) + (0.08 * tint_offset),
                1.0 - (0.18 * abs(tint_offset)),
                1.0 - (0.28 * temperature_offset) + (0.08 * tint_offset),
            ],
            dtype=np.float32,
        )
        pixels *= exposure_gain * gains
        rendered = Image.fromarray(np.clip(pixels, 0, 255).astype(np.uint8), mode="RGB")
        output = io.BytesIO()
        rendered.save(output, format="JPEG", quality=88, optimize=True)
        payload = output.getvalue()
        with self._cache_lock:
            self._cache[cache_key] = payload
            self._cache.move_to_end(cache_key)
            while len(self._cache) > self.cache_entries:
                self._cache.popitem(last=False)
        return payload

    def _move_history(self, photo_id: str, *, undo: bool) -> dict[str, object]:
        with self.database.session() as session:
            photo, document = self._get_rows(session, photo_id)
            source_stack = self._stack(document.undo_stack_json if undo else document.redo_stack_json)
            if not source_stack:
                raise ValueError("Nothing to undo" if undo else "Nothing to redo")
            current = self._recipe(document)
            restored = source_stack.pop()
            destination_stack = self._stack(document.redo_stack_json if undo else document.undo_stack_json)
            destination_stack.append(current)
            if undo:
                document.undo_stack_json = json.dumps(source_stack, sort_keys=True)
                document.redo_stack_json = json.dumps(destination_stack[-50:], sort_keys=True)
            else:
                document.redo_stack_json = json.dumps(source_stack, sort_keys=True)
                document.undo_stack_json = json.dumps(destination_stack[-50:], sort_keys=True)
            document.recipe_json = json.dumps(restored, sort_keys=True)
            document.revision += 1
            document.updated_at = utc_now()
            dto = self._to_dto(photo.photo_id, document)
        self._invalidate(photo_id)
        return dto

    def _resolve_source(self, photo_id: str) -> Path:
        with self.database.session() as session:
            photo = session.execute(select(PhotoDB).where(PhotoDB.photo_id == photo_id)).scalar_one_or_none()
            if photo is None:
                raise LookupError("Photo not found")
            files = (
                session.execute(
                    select(FileDB)
                    .where(FileDB.photo_id == photo.id, FileDB.status == "present")
                    .order_by(
                        (FileDB.role == "jpeg").desc(),
                        (FileDB.role == "image").desc(),
                    )
                )
                .scalars()
                .all()
            )
            for file_row in files:
                path = Path(file_row.relative_path)
                if path.is_file():
                    return path
        raise ValueError("Photo source is unavailable")

    @staticmethod
    def _get_rows(session, photo_id: str) -> tuple[PhotoDB, EditDocumentDB]:
        photo = session.execute(select(PhotoDB).where(PhotoDB.photo_id == photo_id)).scalar_one_or_none()
        if photo is None:
            raise LookupError("Photo not found")
        document = session.execute(
            select(EditDocumentDB).where(EditDocumentDB.photo_id == photo.id)
        ).scalar_one_or_none()
        if document is None:
            document = EditDocumentDB(
                id=str(uuid.uuid4()),
                photo_id=photo.id,
                recipe_json=json.dumps(DEFAULT_RECIPE, sort_keys=True),
            )
            session.add(document)
            session.flush()
        return photo, document

    @staticmethod
    def _recipe(document: EditDocumentDB) -> dict[str, float | int]:
        stored = json.loads(document.recipe_json or "{}")
        return {**DEFAULT_RECIPE, **stored}

    @staticmethod
    def _stack(value: str) -> list[dict[str, float | int]]:
        return list(json.loads(value or "[]"))

    @classmethod
    def _to_dto(cls, photo_id: str, document: EditDocumentDB) -> dict[str, object]:
        return {
            "contract_version": document.contract_version,
            "id": document.id,
            "photo_id": photo_id,
            "revision": document.revision,
            "recipe": cls._recipe(document),
            "can_undo": bool(cls._stack(document.undo_stack_json)),
            "can_redo": bool(cls._stack(document.redo_stack_json)),
            "updated_at": document.updated_at.isoformat(),
        }

    def _invalidate(self, photo_id: str) -> None:
        with self._cache_lock:
            stale_keys = [key for key in self._cache if key[0] == photo_id]
            for key in stale_keys:
                self._cache.pop(key, None)
