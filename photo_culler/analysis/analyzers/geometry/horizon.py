"""Conservative horizon/level estimate from strong near-horizontal edges."""

from __future__ import annotations

import numpy as np

from ...engine.analyzer import Analyzer
from ...engine.context import AnalysisContext
from ...engine.result import AnalysisResult


class HorizonAnalyzer(Analyzer):
    """Estimate a small rotation correction; never rotates an image itself."""

    name = "horizon"
    version = "0.1"
    category = "geometry"

    def analyze(self, context: AnalysisContext) -> AnalysisResult:
        array = context.get_analysis_array(max_dim=1600).astype(np.float32)
        luminance = 0.2126 * array[:, :, 0] + 0.7152 * array[:, :, 1] + 0.0722 * array[:, :, 2]
        gy, gx = np.gradient(luminance)
        magnitude = np.hypot(gx, gy)
        # A horizon produces a mostly vertical gradient. Exclude weak texture.
        mask = (np.abs(gy) > np.abs(gx) * 1.8) & (magnitude >= np.percentile(magnitude, 92))
        y, x = np.nonzero(mask)
        if len(x) < max(40, array.shape[1] // 12):
            return AnalysisResult(
                analyzer=self.name,
                version=self.version,
                metrics={"has_reliable_horizon": False, "recommended_rotation_degrees": 0.0},
                confidence=0.0,
            )

        weights = magnitude[mask]
        slope, intercept = np.polyfit(x, y, 1, w=weights)
        predicted = slope * x + intercept
        residual = float(np.average((y - predicted) ** 2, weights=weights))
        spread = float(np.average((y - np.average(y, weights=weights)) ** 2, weights=weights))
        fit = max(0.0, min(1.0, 1.0 - residual / (spread + 1e-6)))
        correction = float(np.clip(-np.degrees(np.arctan(slope)), -15.0, 15.0))
        reliable = fit >= 0.45 and abs(correction) >= 0.25
        confidence = round(fit * min(1.0, len(x) / (array.shape[1] * 0.35)), 3)
        return AnalysisResult(
            analyzer=self.name,
            version=self.version,
            metrics={
                "has_reliable_horizon": reliable,
                "horizon_angle_degrees": round(-correction, 2),
                "recommended_rotation_degrees": round(correction if reliable else 0.0, 2),
                "horizon_fit": round(fit, 3),
            },
            confidence=confidence,
        )
