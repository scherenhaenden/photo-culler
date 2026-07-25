"""Histogram Analyzer for computing color and luminance distribution statistics."""

import numpy as np

from ...engine.analyzer import Analyzer
from ...engine.context import AnalysisContext
from ...engine.result import AnalysisResult


class HistogramAnalyzer(Analyzer):
    """Calculates channel histograms, percentiles, median brightness, and entropy."""

    name = "histogram"
    version = "1.0"
    category = "technical"

    def analyze(self, context: AnalysisContext) -> AnalysisResult:
        arr = context.get_numpy_array()

        # Calculate per-channel histograms (256 bins)
        r_hist, _ = np.histogram(arr[:, :, 0], bins=256, range=(0, 256))
        g_hist, _ = np.histogram(arr[:, :, 1], bins=256, range=(0, 256))
        b_hist, _ = np.histogram(arr[:, :, 2], bins=256, range=(0, 256))

        # Standard Rec.709 luminance conversion
        lum = 0.2126 * arr[:, :, 0] + 0.7152 * arr[:, :, 1] + 0.0722 * arr[:, :, 2]
        lum_hist, _ = np.histogram(lum, bins=256, range=(0, 256))

        total_pixels = lum.size
        p1 = float(np.percentile(lum, 1))
        p99 = float(np.percentile(lum, 99))
        median_lum = float(np.median(lum))
        mean_lum = float(np.mean(lum))
        std_lum = float(np.std(lum))

        # Compute Shannon Entropy on normalized luminance histogram
        lum_probs = lum_hist / total_pixels
        nonzero_probs = lum_probs[lum_probs > 0]
        entropy = float(-np.sum(nonzero_probs * np.log2(nonzero_probs)))

        # Cache shared histogram features for downstream analyzers
        context.shared_features["histogram"] = {
            "r_hist": r_hist,
            "g_hist": g_hist,
            "b_hist": b_hist,
            "lum_hist": lum_hist,
            "luminance": lum,
        }

        metrics = {
            "percentile_1": round(p1, 2),
            "percentile_99": round(p99, 2),
            "median_luminance": round(median_lum, 2),
            "mean_luminance": round(mean_lum, 2),
            "std_luminance": round(std_lum, 2),
            "entropy": round(entropy, 4),
            "r_mean": round(float(np.mean(arr[:, :, 0])), 2),
            "g_mean": round(float(np.mean(arr[:, :, 1])), 2),
            "b_mean": round(float(np.mean(arr[:, :, 2])), 2),
        }

        return AnalysisResult(
            analyzer=self.name,
            version=self.version,
            metrics=metrics,
            confidence=1.0,
        )
