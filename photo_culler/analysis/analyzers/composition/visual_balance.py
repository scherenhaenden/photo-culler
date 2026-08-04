"""Suggest a crop from visual weight, without claiming to identify a subject."""

from __future__ import annotations

import numpy as np

from ...engine.analyzer import Analyzer
from ...engine.context import AnalysisContext
from ...engine.result import AnalysisResult


class VisualBalanceAnalyzer(Analyzer):
    """Find whether high-detail visual weight is far from the frame centre.

    It is intentionally described as *visual weight*, not person/object
    detection: faces, horizons and semantics require a separately evaluated
    local model before they can be used as crop authority.
    """

    name = "visual_balance"
    version = "0.1"
    category = "composition"

    def analyze(self, context: AnalysisContext) -> AnalysisResult:
        array = context.get_analysis_array(max_dim=1600).astype(np.float32)
        luminance = 0.2126 * array[:, :, 0] + 0.7152 * array[:, :, 1] + 0.0722 * array[:, :, 2]
        gy, gx = np.gradient(luminance)
        energy = np.hypot(gx, gy)
        cutoff = np.percentile(energy, 85)
        weights = np.where(energy >= cutoff, energy, 0.0)
        total = float(weights.sum())
        if total <= 1e-6:
            return AnalysisResult(self.name, self.version, {"crop_recommended": False}, confidence=0.0)

        height, width = luminance.shape
        yy, xx = np.indices(luminance.shape)
        center_x = float((weights * xx).sum() / total / max(1, width - 1))
        center_y = float((weights * yy).sum() / total / max(1, height - 1))
        offset = float(np.hypot(center_x - 0.5, center_y - 0.5))
        crop_recommended = offset >= 0.18
        # An 85% crop is deliberately small; it is a review starting point.
        crop_w = crop_h = 0.85
        crop_x = float(np.clip(center_x - crop_w / 2, 0.0, 1.0 - crop_w))
        crop_y = float(np.clip(center_y - crop_h / 2, 0.0, 1.0 - crop_h))
        return AnalysisResult(
            analyzer=self.name,
            version=self.version,
            metrics={
                "visual_weight_x": round(center_x, 3),
                "visual_weight_y": round(center_y, 3),
                "visual_weight_offset": round(offset, 3),
                "crop_recommended": crop_recommended,
                "suggested_crop_normalized": {
                    "x": round(crop_x, 3), "y": round(crop_y, 3), "width": crop_w, "height": crop_h
                },
            },
            confidence=round(min(1.0, offset / 0.35), 3),
        )
