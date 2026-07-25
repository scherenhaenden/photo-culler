"""Exposure Analyzer for evaluating exposure balance, underexposure, and overexposure probabilities."""

import numpy as np
from ...engine.analyzer import Analyzer
from ...engine.context import AnalysisContext
from ...engine.result import AnalysisResult


class ExposureAnalyzer(Analyzer):
    """Calculates exposure score, underexposed probability, overexposed probability, and balance."""

    name = "exposure"
    version = "1.0"
    category = "technical"

    def analyze(self, context: AnalysisContext) -> AnalysisResult:
        if "histogram" in context.shared_features:
            lum_hist = context.shared_features["histogram"]["lum_hist"]
            total_pixels = lum_hist.sum()
            probs = lum_hist / total_pixels
            bins = np.arange(256)
            mean_lum = float(np.sum(bins * probs))
            
            # Probability of underexposure: weight of pixels below bin 60
            under_prob = float(np.sum(probs[:60]))
            # Probability of overexposure: weight of pixels above bin 195
            over_prob = float(np.sum(probs[195:]))
        else:
            arr = context.get_numpy_array()
            lum = 0.2126 * arr[:, :, 0] + 0.7152 * arr[:, :, 1] + 0.0722 * arr[:, :, 2]
            mean_lum = float(np.mean(lum))
            under_prob = float(np.mean(lum < 60))
            over_prob = float(np.mean(lum > 195))

        # Ideal target mean luminance ~118 (out of 255)
        # exposure_score: 1.0 is perfectly balanced, drops as mean lum drifts from target
        deviation = abs(mean_lum - 118.0) / 118.0
        exposure_score = max(0.0, min(1.0, 1.0 - deviation))

        is_underexposed = under_prob > 0.40 and mean_lum < 80
        is_overexposed = over_prob > 0.40 and mean_lum > 175
        is_balanced = not is_underexposed and not is_overexposed

        metrics = {
            "exposure_score": round(exposure_score, 4),
            "mean_luminance": round(mean_lum, 2),
            "underexposed_probability": round(under_prob, 4),
            "overexposed_probability": round(over_prob, 4),
            "is_underexposed": is_underexposed,
            "is_overexposed": is_overexposed,
            "is_balanced": is_balanced,
        }

        return AnalysisResult(
            analyzer=self.name,
            version=self.version,
            metrics=metrics,
            confidence=0.95,
        )
