"""Technical Quality Scorer for aggregating individual analyzer measurements."""

from typing import Dict, Any
from photo_culler.analysis.engine.result import AnalysisResult


class TechnicalScorer:
    """Aggregates raw measurements into a composite technical quality score (0.0 - 1.0)."""

    def __init__(
        self,
        weight_sharpness: float = 0.40,
        weight_exposure: float = 0.25,
        weight_clipping: float = 0.20,
        weight_noise: float = 0.15,
    ):
        self.w_sharpness = weight_sharpness
        self.w_exposure = weight_exposure
        self.w_clipping = weight_clipping
        self.w_noise = weight_noise

    def calculate_score(self, results: Dict[str, AnalysisResult]) -> Dict[str, Any]:
        """Compute final composite score and component breakdowns."""
        # 1. Hard fail on corruption
        if "corruption" in results:
            corr_metrics = results["corruption"].metrics
            if corr_metrics.get("corruption_status") != "healthy":
                return {
                    "final_score": 0.0,
                    "quality_tier": "corrupted",
                    "reason": "File is corrupted or unreadable",
                    "components": {},
                }

        # 2. Sharpness component (0.0 to 1.0)
        sharpness_score = 0.5
        if "sharpness" in results:
            sharpness_score = float(results["sharpness"].metrics.get("global_sharpness", 0.5))

        # 3. Exposure component (0.0 to 1.0)
        exposure_score = 0.5
        if "exposure" in results:
            exposure_score = float(results["exposure"].metrics.get("exposure_score", 0.5))

        # 4. Clipping component (1.0 = zero clipping, drops with blown highlights/crushed shadows)
        clipping_score = 1.0
        if "clipping" in results:
            clip_m = results["clipping"].metrics
            hi_pct = float(clip_m.get("highlight_clipping_pct", 0.0))
            sh_pct = float(clip_m.get("shadow_clipping_pct", 0.0))
            penalty = (hi_pct * 0.02) + (sh_pct * 0.01)
            clipping_score = max(0.0, 1.0 - penalty)

        # 5. Noise component (1.0 = clean, 0.0 = high noise)
        noise_score = 0.8
        if "noise" in results:
            est_noise = float(results["noise"].metrics.get("estimated_noise_level", 0.1))
            noise_score = max(0.0, 1.0 - est_noise)

        # Weighted aggregate score
        final_score = (
            (sharpness_score * self.w_sharpness) +
            (exposure_score * self.w_exposure) +
            (clipping_score * self.w_clipping) +
            (noise_score * self.w_noise)
        )
        final_score = round(max(0.0, min(1.0, final_score)), 4)

        if final_score >= 0.80:
            quality_tier = "excellent"
        elif final_score >= 0.60:
            quality_tier = "good"
        elif final_score >= 0.40:
            quality_tier = "fair"
        else:
            quality_tier = "poor"

        return {
            "final_score": final_score,
            "quality_tier": quality_tier,
            "components": {
                "sharpness": round(sharpness_score, 4),
                "exposure": round(exposure_score, 4),
                "clipping": round(clipping_score, 4),
                "noise": round(noise_score, 4),
            }
        }
