"""Build human-readable, persisted explanations for technical scores."""

from __future__ import annotations

from datetime import datetime, timezone
from typing import Any

from photo_culler.analysis.engine.result import AnalysisResult

_COMPONENTS = {
    "sharpness": ("Nitidez", "sharpness"),
    "exposure": ("Exposición", "exposure"),
    "clipping": ("Luces y sombras", "clipping"),
    "noise": ("Ruido", "noise"),
}


def build_score_explanation(
    profile: dict[str, Any], score: dict[str, Any], results: dict[str, AnalysisResult]
) -> dict[str, Any]:
    """Return the exact weighted calculation plus the measurements behind it."""
    weights = profile["weights"]
    active_keys = [
        key
        for key, (_, analyzer) in _COMPONENTS.items()
        if analyzer in results and not results[analyzer].error and float(weights.get(key, 0)) > 0
    ]
    active_weight = sum(float(weights[key]) for key in active_keys)
    components = []
    for key in active_keys:
        label, analyzer = _COMPONENTS[key]
        component_score = float(score["components"][key])
        effective_weight = float(weights[key]) / active_weight if active_weight else 0.0
        metrics = results[analyzer].metrics
        components.append(
            {
                "id": key,
                "label": label,
                "score_percent": round(component_score * 100, 1),
                "weight_percent": round(effective_weight * 100, 1),
                "contribution_points": round(component_score * effective_weight * 100, 1),
                "measurement": _measurement(key, metrics),
            }
        )

    return {
        "profile_id": profile["id"],
        "profile_name": profile["name"],
        "final_score_percent": round(float(score["final_score"]) * 100, 1),
        "quality_tier": score["quality_tier"],
        "confidence_percent": round(float(score["overall_confidence"]) * 100, 1),
        "components": components,
        "formula": " + ".join(
            f"{component['contribution_points']:g} {component['label'].lower()}" for component in components
        ),
        "analyzed_at": datetime.now(timezone.utc).isoformat(),
    }


def _measurement(component: str, metrics: dict[str, Any]) -> str:
    if component == "sharpness":
        focus = float(metrics.get("effective_focus_variance", 0))
        state = "suave" if metrics.get("is_soft") else "nítida" if metrics.get("is_tack_sharp") else "aceptable"
        return f"Foco {state}: varianza efectiva {focus:.1f}."
    if component == "exposure":
        mean = float(metrics.get("mean_luminance", 0))
        under = float(metrics.get("underexposed_probability", 0)) * 100
        over = float(metrics.get("overexposed_probability", 0)) * 100
        return f"Luminancia media {mean:.0f}/255; sombras {under:.1f}%, luces {over:.1f}%."
    if component == "clipping":
        highlights = float(metrics.get("highlight_clipping_pct", 0))
        shadows = float(metrics.get("shadow_clipping_pct", 0))
        center = float(metrics.get("center_highlight_clipping_pct", 0))
        return f"Recorte: luces {highlights:.2f}%, sombras {shadows:.2f}%, centro {center:.2f}%."
    noise = float(metrics.get("estimated_noise_level", 0)) * 100
    return f"Ruido estimado {noise:.1f}% ({'alto' if metrics.get('is_noisy') else 'bajo'})."
