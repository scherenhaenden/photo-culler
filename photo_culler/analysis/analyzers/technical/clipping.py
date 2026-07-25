"""Clipping Analyzer for measuring highlight blown-out pixels and shadow crushed pixels."""

import numpy as np

from ...engine.analyzer import Analyzer
from ...engine.context import AnalysisContext
from ...engine.result import AnalysisResult


class ClippingAnalyzer(Analyzer):
    """Calculates highlight clipping % and shadow clipping % globally and per channel."""

    name = "clipping"
    version = "1.0"
    category = "technical"

    def analyze(self, context: AnalysisContext) -> AnalysisResult:
        if "histogram" in context.shared_features:
            lum_hist = context.shared_features["histogram"]["lum_hist"]
            r_hist = context.shared_features["histogram"]["r_hist"]
            g_hist = context.shared_features["histogram"]["g_hist"]
            b_hist = context.shared_features["histogram"]["b_hist"]
            total_pixels = lum_hist.sum()

            shadow_clipped = float(lum_hist[0] / total_pixels)
            highlight_clipped = float(lum_hist[255] / total_pixels)

            r_shadow = float(r_hist[0] / total_pixels)
            r_highlight = float(r_hist[255] / total_pixels)
            g_shadow = float(g_hist[0] / total_pixels)
            g_highlight = float(g_hist[255] / total_pixels)
            b_shadow = float(b_hist[0] / total_pixels)
            b_highlight = float(b_hist[255] / total_pixels)
        else:
            arr = context.get_numpy_array()
            total_pixels = arr.shape[0] * arr.shape[1]

            lum = 0.2126 * arr[:, :, 0] + 0.7152 * arr[:, :, 1] + 0.0722 * arr[:, :, 2]
            shadow_clipped = float(np.sum(lum <= 1) / total_pixels)
            highlight_clipped = float(np.sum(lum >= 254) / total_pixels)

            r_shadow = float(np.sum(arr[:, :, 0] <= 1) / total_pixels)
            r_highlight = float(np.sum(arr[:, :, 0] >= 254) / total_pixels)
            g_shadow = float(np.sum(arr[:, :, 1] <= 1) / total_pixels)
            g_highlight = float(np.sum(arr[:, :, 1] >= 254) / total_pixels)
            b_shadow = float(np.sum(arr[:, :, 2] <= 1) / total_pixels)
            b_highlight = float(np.sum(arr[:, :, 2] >= 254) / total_pixels)

        metrics = {
            "highlight_clipping_pct": round(highlight_clipped * 100, 3),
            "shadow_clipping_pct": round(shadow_clipped * 100, 3),
            "r_highlight_pct": round(r_highlight * 100, 3),
            "r_shadow_pct": round(r_shadow * 100, 3),
            "g_highlight_pct": round(g_highlight * 100, 3),
            "g_shadow_pct": round(g_shadow * 100, 3),
            "b_highlight_pct": round(b_highlight * 100, 3),
            "b_shadow_pct": round(b_shadow * 100, 3),
            "has_severe_clipping": highlight_clipped > 0.10 or shadow_clipped > 0.15,
        }

        return AnalysisResult(
            analyzer=self.name,
            version=self.version,
            metrics=metrics,
            confidence=1.0,
        )
