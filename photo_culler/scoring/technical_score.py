"""Technical Quality Scorer for aggregating individual analyzer measurements with context awareness and explicit measurement confidence."""

from typing import Any, Dict

from photo_culler.analysis.engine.result import AnalysisResult


class TechnicalScorer:
    """Aggregates raw measurements into a composite technical quality score (0.0 - 1.0) and confidence score."""

    def __init__(
        self,
        profile: str = "default",
        weight_sharpness: float = 0.40,
        weight_exposure: float = 0.25,
        weight_clipping: float = 0.20,
        weight_noise: float = 0.15,
    ):
        self.profile = profile
        self.w_sharpness = weight_sharpness
        self.w_exposure = weight_exposure
        self.w_clipping = weight_clipping
        self.w_noise = weight_noise

    def calculate_score(self, results: Dict[str, AnalysisResult]) -> Dict[str, Any]:
        """Compute composite score, component breakdowns, and overall confidence score."""
        # 1. Hard fail on corruption
        if "corruption" in results:
            corr_metrics = results["corruption"].metrics
            if corr_metrics.get("corruption_status") != "healthy":
                return {
                    "final_score": 0.0,
                    "quality_tier": "corrupted",
                    "reason": "File is corrupted or unreadable",
                    "overall_confidence": 1.0,
                    "components": {},
                }

        confidence_sum = 0.0
        analyzers_present = 0

        # 2. Sharpness component (0.0 to 1.0)
        sharpness_score = 0.0
        if "sharpness" in results and not results["sharpness"].error:
            res = results["sharpness"]
            sharpness_score = float(res.metrics.get("global_sharpness", 0.5))
            confidence_sum += res.confidence
            analyzers_present += 1

        # 3. Exposure component (0.0 to 1.0)
        exposure_score = 0.0
        if "exposure" in results and not results["exposure"].error:
            res = results["exposure"]
            exposure_score = float(res.metrics.get("exposure_score", 0.5))
            confidence_sum += res.confidence
            analyzers_present += 1

        # 4. Clipping component
        clipping_score = 1.0
        if "clipping" in results and not results["clipping"].error:
            res = results["clipping"]
            clip_m = res.metrics
            hi_pct = float(clip_m.get("highlight_clipping_pct", 0.0))
            center_hi_pct = float(clip_m.get("center_highlight_clipping_pct", hi_pct))
            sh_pct = float(clip_m.get("shadow_clipping_pct", 0.0))

            if self.profile == "concert":
                # In concerts, background stage lights (hi_pct) are expected; penalize subject center clipping heavier
                penalty = (center_hi_pct * 0.03) + (hi_pct * 0.005) + (sh_pct * 0.01)
            else:
                penalty = (hi_pct * 0.02) + (sh_pct * 0.01)

            clipping_score = max(0.0, 1.0 - penalty)
            confidence_sum += res.confidence
            analyzers_present += 1

        # 5. Noise component
        noise_score = 1.0
        if "noise" in results and not results["noise"].error:
            res = results["noise"]
            est_noise = float(res.metrics.get("estimated_noise_level", 0.1))
            noise_score = max(0.0, 1.0 - est_noise)
            confidence_sum += res.confidence
            analyzers_present += 1

        # Overall measurement confidence ratio
        overall_confidence = round(confidence_sum / max(1, analyzers_present), 4) if analyzers_present > 0 else 0.0

        # Weighted aggregate score
        final_score = (
            (sharpness_score * self.w_sharpness)
            + (exposure_score * self.w_exposure)
            + (clipping_score * self.w_clipping)
            + (noise_score * self.w_noise)
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
            "overall_confidence": overall_confidence,
            "profile_used": self.profile,
            "components": {
                "sharpness": round(sharpness_score, 4),
                "exposure": round(exposure_score, 4),
                "clipping": round(clipping_score, 4),
                "noise": round(noise_score, 4),
            },
        }
