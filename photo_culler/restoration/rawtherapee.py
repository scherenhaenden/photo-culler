"""Explainable RawTherapee profile suggestions from culling measurements.

This is deliberately a *suggestion* layer.  It does not decode a RAW, write a
sidecar next to one, or attempt AI restoration.  The caller chooses whether to
save the generated ``.pp3`` text and apply it in RawTherapee.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Mapping


@dataclass(frozen=True)
class RawTherapeeSuggestion:
    """A reviewable profile proposal and the evidence used to make it."""

    pp3: str
    reasons: tuple[str, ...]
    warnings: tuple[str, ...]
    confidence: float
    geometry: Mapping[str, Any]


class RawTherapeeProfileSuggester:
    """Convert existing technical metrics into conservative ``.pp3`` settings.

    The thresholds are intentionally modest.  Technical measurements made from
    an embedded JPEG/preview are useful for triage, but are not ground truth
    for the RAW's highlight headroom or detail.  Directional motion blur is
    never sharpened automatically because conventional RAW processing cannot
    reliably reconstruct it.
    """

    VERSION = "0.1"

    def suggest(self, results: Mapping[str, Any]) -> RawTherapeeSuggestion:
        """Return a `.pp3` proposal from analyzer results or plain metric maps.

        ``results`` accepts either ``{"exposure": {…}}`` or the pipeline's
        ``{"exposure": AnalysisResult(…)}`` shape.  Missing analyzers leave
        their tools disabled and lower the reported confidence.
        """
        exposure = self._metrics(results, "exposure")
        clipping = self._metrics(results, "clipping")
        noise = self._metrics(results, "noise")
        sharpness = self._metrics(results, "sharpness")
        motion = self._metrics(results, "motion_blur")
        horizon = self._metrics(results, "horizon")
        balance = self._metrics(results, "visual_balance")

        reasons: list[str] = []
        warnings: list[str] = [
            "Propuesta experimental: revísala al 100% en RawTherapee antes de exportar.",
            "El RAW original no se modifica; guarda el .pp3 solo si decides aplicarlo.",
        ]

        compensation = 0.0
        mean_luminance = self._number(exposure, "mean_luminance")
        underexposed = bool(exposure.get("is_underexposed", False))
        overexposed = bool(exposure.get("is_overexposed", False))
        if underexposed or (mean_luminance is not None and mean_luminance < 80):
            # A bounded starting point, not an exposure correction guarantee.
            compensation = round(min(1.0, max(0.25, (118.0 - (mean_luminance or 80.0)) / 75.0)), 2)
            reasons.append(f"Luminancia baja: propone +{compensation:.2f} EV como punto de partida.")
        elif overexposed or (mean_luminance is not None and mean_luminance > 175):
            compensation = round(max(-1.0, min(-0.25, (118.0 - (mean_luminance or 175.0)) / 75.0)), 2)
            reasons.append(f"Luminancia alta: propone {compensation:.2f} EV como punto de partida.")

        highlight = self._number(clipping, "highlight_clipping_pct") or 0.0
        center_highlight = self._number(clipping, "center_highlight_clipping_pct") or 0.0
        shadow = self._number(clipping, "shadow_clipping_pct") or 0.0
        highlight_compression = 0
        shadow_compression = 0
        if max(highlight, center_highlight) >= 2.0:
            highlight_compression = min(100, round(35 + max(highlight, center_highlight) * 3))
            reasons.append("Hay altas luces recortadas: activa compresión de altas luces.")
        if shadow >= 8.0:
            shadow_compression = min(100, round(20 + shadow * 2))
            reasons.append("Hay sombras recortadas: propone recuperación moderada de sombras.")

        noise_level = self._number(noise, "estimated_noise_level") or 0.0
        luma = chroma = ldetail = 0
        if noise_level >= 0.12:
            luma = min(50, round(10 + noise_level * 55))
            chroma = min(50, round(8 + noise_level * 42))
            ldetail = max(20, 70 - luma)
            reasons.append("El ruido medido activa reducción de ruido conservadora en Lab.")

        directional_motion = bool(motion.get("is_directional_motion_blur", False))
        is_soft = bool(sharpness.get("is_soft", False))
        sharpen = not directional_motion and not is_soft and noise_level < 0.35
        if directional_motion:
            warnings.append("Se detectó blur direccional: no se aplica enfoque, pues puede crear halos.")
        elif is_soft:
            warnings.append("La toma parece fuera de foco: no se propone enfoque para no inventar detalle.")
        elif sharpen:
            reasons.append("La toma tiene foco aprovechable: activa enfoque suave solo en bordes.")

        geometry: dict[str, Any] = {}
        rotation = self._number(horizon, "recommended_rotation_degrees") or 0.0
        if bool(horizon.get("has_reliable_horizon", False)) and abs(rotation) >= 0.25:
            geometry["rotation_degrees"] = rotation
            reasons.append(f"Horizonte inclinado: propone girar {rotation:+.2f}° antes de recortar.")
        elif horizon:
            warnings.append("No hay una línea de horizonte suficientemente fiable para enderezar automáticamente.")

        crop = balance.get("suggested_crop_normalized")
        if bool(balance.get("crop_recommended", False)) and isinstance(crop, Mapping):
            geometry["crop_normalized"] = dict(crop)
            reasons.append("El peso visual está descentrado: propone un recorte leve para revisión.")
        elif balance:
            reasons.append("El peso visual está suficientemente equilibrado: no propone recorte.")

        available = sum(bool(section) for section in (exposure, clipping, noise, sharpness, motion, horizon, balance))
        confidence = round(available / 7.0, 2)
        if available < 5:
            warnings.append("Faltan métricas; ejecuta el perfil Technical Precision para una propuesta completa.")

        pp3 = self._render_pp3(
            compensation=compensation,
            highlight_compression=highlight_compression,
            shadow_compression=shadow_compression,
            luma=luma,
            chroma=chroma,
            ldetail=ldetail,
            sharpen=sharpen,
        )
        return RawTherapeeSuggestion(pp3, tuple(reasons), tuple(warnings), confidence, geometry)

    @staticmethod
    def _metrics(results: Mapping[str, Any], name: str) -> Mapping[str, Any]:
        result = results.get(name, {})
        metrics = getattr(result, "metrics", result)
        return metrics if isinstance(metrics, Mapping) else {}

    @staticmethod
    def _number(metrics: Mapping[str, Any], name: str) -> float | None:
        value = metrics.get(name)
        return float(value) if isinstance(value, int | float) else None

    @staticmethod
    def _render_pp3(**settings: int | float | bool) -> str:
        """Render only settings owned by this experimental profile.

        Section/key names match the installed RawTherapee 5.9 profile format.
        Keeping this partial makes it safe to merge with a photographer's own
        camera, lens, and color-management defaults.
        """
        enabled = "true" if settings["sharpen"] else "false"
        denoise_enabled = "true" if settings["luma"] or settings["chroma"] else "false"
        return f"""# Photo Culler experimental suggestion v{RawTherapeeProfileSuggester.VERSION}
# Review visually before use. Generated from preview metrics, not RAW pixels.
[Exposure]
Auto=false
Compensation={settings['compensation']:.2f}
HighlightCompr={settings['highlight_compression']}
ShadowCompr={settings['shadow_compression']}

[Directional Pyramid Denoising]
Enabled={denoise_enabled}
Luma={settings['luma']}
Chroma={settings['chroma']}
Ldetail={settings['ldetail']}
Method=Lab

[Sharpening]
Enabled={enabled}
Amount=150
Radius=0.60
OnlyEdges=true
"""
