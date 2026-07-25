"""Clipping Analyzer for measuring global and center subject highlight/shadow clipping."""

import numpy as np

from ...engine.analyzer import Analyzer
from ...engine.context import AnalysisContext
from ...engine.result import AnalysisResult


class ClippingAnalyzer(Analyzer):
    """Calculates highlight clipping % and shadow clipping % globally and for central subject region."""

    name = "clipping"
    version = "1.1"
    category = "technical"

    def analyze(self, context: AnalysisContext) -> AnalysisResult:
        arr = context.get_analysis_array(max_dim=1920)
        total_pixels = arr.shape[0] * arr.shape[1]

        lum = 0.2126 * arr[:, :, 0] + 0.7152 * arr[:, :, 1] + 0.0722 * arr[:, :, 2]
        shadow_clipped = float(np.sum(lum <= 1) / total_pixels)
        highlight_clipped = float(np.sum(lum >= 254) / total_pixels)

        # Central 50% region clipping (subject zone)
        h, w = lum.shape
        cy_start, cy_end = int(h * 0.25), int(h * 0.75)
        cx_start, cx_end = int(w * 0.25), int(w * 0.75)
        center_lum = lum[cy_start:cy_end, cx_start:cx_end]
        center_pixels = center_lum.size

        center_highlight_clipped = float(np.sum(center_lum >= 254) / center_pixels)
        center_shadow_clipped = float(np.sum(center_lum <= 1) / center_pixels)

        r_shadow = float(np.sum(arr[:, :, 0] <= 1) / total_pixels)
        r_highlight = float(np.sum(arr[:, :, 0] >= 254) / total_pixels)
        g_shadow = float(np.sum(arr[:, :, 1] <= 1) / total_pixels)
        g_highlight = float(np.sum(arr[:, :, 1] >= 254) / total_pixels)
        b_shadow = float(np.sum(arr[:, :, 2] <= 1) / total_pixels)
        b_highlight = float(np.sum(arr[:, :, 2] >= 254) / total_pixels)

        metrics = {
            "highlight_clipping_pct": round(highlight_clipped * 100, 3),
            "shadow_clipping_pct": round(shadow_clipped * 100, 3),
            "center_highlight_clipping_pct": round(center_highlight_clipped * 100, 3),
            "center_shadow_clipping_pct": round(center_shadow_clipped * 100, 3),
            "r_highlight_pct": round(r_highlight * 100, 3),
            "r_shadow_pct": round(r_shadow * 100, 3),
            "g_highlight_pct": round(g_highlight * 100, 3),
            "g_shadow_pct": round(g_shadow * 100, 3),
            "b_highlight_pct": round(b_highlight * 100, 3),
            "b_shadow_pct": round(b_shadow * 100, 3),
            "has_severe_subject_clipping": center_highlight_clipped > 0.08 or center_shadow_clipped > 0.12,
        }

        return AnalysisResult(
            analyzer=self.name,
            version=self.version,
            metrics=metrics,
            confidence=1.0,
        )
