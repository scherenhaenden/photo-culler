"""Persistent, user-editable analysis profile definitions."""

from __future__ import annotations

import hashlib
import json
import os
import re
import threading
from copy import deepcopy
from pathlib import Path
from typing import Any

ANALYZER_CATALOG: tuple[dict[str, str], ...] = (
    {
        "id": "corruption",
        "name": "Integridad del archivo",
        "description": "Comprueba que la imagen pueda abrirse y decodificarse sin errores.",
        "cost": "Muy bajo",
    },
    {
        "id": "dimensions",
        "name": "Dimensiones y orientación",
        "description": "Lee resolución, relación de aspecto y orientación del archivo.",
        "cost": "Muy bajo",
    },
    {
        "id": "histogram",
        "name": "Histograma",
        "description": "Resume la distribución tonal y de luminancia de la fotografía.",
        "cost": "Medio",
    },
    {
        "id": "clipping",
        "name": "Clipping",
        "description": "Mide luces quemadas, negros empastados y clipping en la zona central.",
        "cost": "Medio",
    },
    {
        "id": "exposure",
        "name": "Exposición",
        "description": "Evalúa el equilibrio general de exposición y detecta sub/sobreexposición.",
        "cost": "Medio",
    },
    {
        "id": "sharpness",
        "name": "Nitidez regional",
        "description": "Mide foco y detalle global y por regiones mediante varianza Laplaciana.",
        "cost": "Alto",
    },
    {
        "id": "motion_blur",
        "name": "Desenfoque de movimiento",
        "description": "Busca patrones direccionales compatibles con movimiento de cámara o sujeto.",
        "cost": "Alto",
    },
    {
        "id": "noise",
        "name": "Ruido",
        "description": "Estima el ruido visible en zonas con poco detalle y su impacto técnico.",
        "cost": "Alto",
    },
)

ANALYZER_IDS = {item["id"] for item in ANALYZER_CATALOG}
WEIGHT_IDS = ("sharpness", "exposure", "clipping", "noise")

DEFAULT_PROFILES: dict[str, dict[str, Any]] = {
    "fast": {
        "id": "fast",
        "name": "Fast Scan",
        "description": "Control rápido de integridad, tamaño, exposición y nitidez.",
        "analyzers": ["corruption", "dimensions", "exposure", "sharpness"],
        "weights": {"sharpness": 0.65, "exposure": 0.35, "clipping": 0.0, "noise": 0.0},
        "clipping_mode": "standard",
        "builtin": True,
    },
    "technical": {
        "id": "technical",
        "name": "Technical Precision",
        "description": "Evaluación técnica completa de tono, foco, movimiento, ruido e integridad.",
        "analyzers": [item["id"] for item in ANALYZER_CATALOG],
        "weights": {"sharpness": 0.40, "exposure": 0.25, "clipping": 0.20, "noise": 0.15},
        "clipping_mode": "standard",
        "builtin": True,
    },
    "concert": {
        "id": "concert",
        "name": "Concert Stage",
        "description": "Perfil completo que tolera luces de escenario y prioriza el centro de la imagen.",
        "analyzers": [item["id"] for item in ANALYZER_CATALOG],
        "weights": {"sharpness": 0.35, "exposure": 0.15, "clipping": 0.30, "noise": 0.20},
        "clipping_mode": "concert",
        "builtin": True,
    },
}


class AnalysisProfileStore:
    """Store profile overrides beside the catalog without changing its schema."""

    def __init__(self, path: str | Path):
        self.path = Path(path)
        self._lock = threading.RLock()
        self._profiles = self._load()

    def list(self) -> list[dict[str, Any]]:
        with self._lock:
            return [deepcopy(profile) for profile in self._profiles.values()]

    def get(self, profile_id: str) -> dict[str, Any] | None:
        with self._lock:
            profile = self._profiles.get(profile_id)
            return deepcopy(profile) if profile else None

    def save(self, data: dict[str, Any], profile_id: str | None = None) -> dict[str, Any]:
        with self._lock:
            requested_id = profile_id or str(data.get("id", ""))
            clean_id = self._clean_id(requested_id or str(data.get("name", "")))
            if not clean_id:
                raise ValueError("El perfil necesita un nombre válido.")
            if profile_id is None and clean_id in self._profiles:
                raise ValueError("Ya existe un perfil con ese identificador.")
            if profile_id is not None and profile_id not in self._profiles:
                raise KeyError(profile_id)

            existing = self._profiles.get(profile_id or clean_id, {})
            profile = self._validate(
                {
                    **existing,
                    **data,
                    "id": profile_id or clean_id,
                    "builtin": bool(existing.get("builtin", False)),
                }
            )
            self._profiles[profile["id"]] = profile
            self._persist()
            return deepcopy(profile)

    def delete(self, profile_id: str) -> None:
        with self._lock:
            profile = self._profiles.get(profile_id)
            if not profile:
                raise KeyError(profile_id)
            if profile.get("builtin"):
                raise ValueError("Los perfiles base se restauran, no se eliminan.")
            del self._profiles[profile_id]
            self._persist()

    def restore(self, profile_id: str) -> dict[str, Any]:
        with self._lock:
            if profile_id not in DEFAULT_PROFILES:
                raise ValueError("Solo los perfiles base se pueden restaurar.")
            self._profiles[profile_id] = deepcopy(DEFAULT_PROFILES[profile_id])
            self._persist()
            return deepcopy(self._profiles[profile_id])

    @staticmethod
    def fingerprint(profile: dict[str, Any]) -> str:
        """Return a cache namespace that changes whenever effective settings change."""
        effective = {
            "id": profile["id"],
            "analyzers": profile["analyzers"],
            "weights": profile["weights"],
            "clipping_mode": profile["clipping_mode"],
        }
        raw = json.dumps(effective, sort_keys=True, separators=(",", ":")).encode()
        return hashlib.sha256(raw).hexdigest()[:16]

    def _load(self) -> dict[str, dict[str, Any]]:
        profiles = deepcopy(DEFAULT_PROFILES)
        if not self.path.exists():
            return profiles
        try:
            payload = json.loads(self.path.read_text(encoding="utf-8"))
            if not isinstance(payload, dict):
                raise ValueError("El archivo de perfiles debe contener un objeto JSON.")
            profile_items = payload.get("profiles", [])
            if not isinstance(profile_items, list):
                raise ValueError("Los perfiles deben ser una lista JSON.")
            for item in profile_items:
                if not isinstance(item, dict):
                    raise ValueError("Cada perfil debe ser un objeto JSON.")
                profile = self._validate(item)
                profiles[profile["id"]] = profile
        except OSError, ValueError, TypeError, json.JSONDecodeError:
            # A damaged preferences file must not prevent the application starting.
            return profiles
        return profiles

    def _persist(self) -> None:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        temp_path = self.path.with_suffix(self.path.suffix + ".tmp")
        temp_path.write_text(
            json.dumps({"version": 1, "profiles": list(self._profiles.values())}, indent=2, ensure_ascii=False),
            encoding="utf-8",
        )
        os.replace(temp_path, self.path)

    @staticmethod
    def _clean_id(value: str) -> str:
        return re.sub(r"[^a-z0-9-]+", "-", value.strip().lower()).strip("-")[:48]

    @staticmethod
    def _validate(data: dict[str, Any]) -> dict[str, Any]:
        profile_id = AnalysisProfileStore._clean_id(str(data.get("id", "")))
        name = str(data.get("name", "")).strip()[:80]
        description = str(data.get("description", "")).strip()[:300]
        analyzers = list(dict.fromkeys(str(item) for item in data.get("analyzers", [])))
        unknown = set(analyzers) - ANALYZER_IDS
        if not profile_id or not name:
            raise ValueError("El identificador y el nombre son obligatorios.")
        if unknown:
            raise ValueError(f"Analizadores desconocidos: {', '.join(sorted(unknown))}")
        if not analyzers:
            raise ValueError("Selecciona al menos un analizador.")

        supplied_weights = data.get("weights", {})
        weights: dict[str, float] = {}
        for key in WEIGHT_IDS:
            value = float(supplied_weights.get(key, 0.0))
            if value < 0 or value > 1:
                raise ValueError("Los pesos deben estar entre 0 y 1.")
            weights[key] = value
        if sum(weights.values()) <= 0:
            raise ValueError("Al menos un peso del score debe ser mayor que cero.")

        clipping_mode = str(data.get("clipping_mode", "standard"))
        if clipping_mode not in {"standard", "concert"}:
            raise ValueError("Tratamiento de clipping desconocido.")
        return {
            "id": profile_id,
            "name": name,
            "description": description,
            "analyzers": analyzers,
            "weights": weights,
            "clipping_mode": clipping_mode,
            "builtin": bool(data.get("builtin", False)),
        }
