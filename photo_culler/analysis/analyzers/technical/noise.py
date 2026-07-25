"""Noise Analyzer for estimating luminance, chroma, and shadow noise levels."""

import numpy as np

from ...engine.analyzer import Analyzer
from ...engine.context import AnalysisContext
from ...engine.result import AnalysisResult


class NoiseAnalyzer(Analyzer):
    """Calculates luminance noise, chroma noise, and noise estimation in shadow regions."""

    name = "noise"
    version = "1.0"
    category = "technical"

    def analyze(self, context: AnalysisContext) -> AnalysisResult:
        arr = context.get_numpy_array().astype(np.float32)

        # 1. Chroma Noise: Standard deviation of (R-G) and (B-G) color differences
        r_g = arr[:, :, 0] - arr[:, :, 1]
        b_g = arr[:, :, 2] - arr[:, :, 1]
        chroma_noise_std = float((np.std(r_g) + np.std(b_g)) / 2.0)

        # 2. Luminance Noise: High pass difference standard deviation
        lum = 0.2126 * arr[:, :, 0] + 0.7152 * arr[:, :, 1] + 0.0722 * arr[:, :, 2]

        diff_h = np.abs(lum[1:, :] - lum[:-1, :])
        diff_v = np.abs(lum[:, 1:] - lum[:, :-1])
        lum_noise_std = float((np.std(diff_h) + np.std(diff_v)) / 2.0)

        # 3. Shadow Noise: High-frequency difference in dark areas (luminance < 40)
        shadow_mask = lum[:-1, :] < 40
        shadow_diffs = diff_h[shadow_mask]
        if len(shadow_diffs) > 100:
            shadow_noise_std = float(np.std(shadow_diffs))
        else:
            shadow_noise_std = lum_noise_std

        # Bounded noise level score (0.0 = clean, 1.0 = extremely noisy)
        noise_score = min(1.0, max(0.0, (lum_noise_std + chroma_noise_std * 0.5) / 30.0))

        metrics = {
            "luminance_noise_std": round(lum_noise_std, 2),
            "chroma_noise_std": round(chroma_noise_std, 2),
            "shadow_noise_std": round(shadow_noise_std, 2),
            "estimated_noise_level": round(noise_score, 4),
            "is_noisy": noise_score > 0.35,
            "is_clean": noise_score < 0.12,
        }

        return AnalysisResult(
            analyzer=self.name,
            version=self.version,
            metrics=metrics,
            confidence=0.89,
        )
