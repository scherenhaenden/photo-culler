"""Recoverability Scorer evaluating RAW potential and shadow/highlight headroom."""

from typing import Any, Dict

from photo_culler.analysis.engine.result import AnalysisResult


class RecoverabilityScorer:
    """Evaluates how much highlight/shadow detail can be restored in editing."""

    def calculate_recoverability(self, results: Dict[str, AnalysisResult]) -> Dict[str, Any]:
        highlight_potential = 0.8
        shadow_potential = 0.9

        if "clipping" in results:
            m = results["clipping"].metrics
            hi_pct = float(m.get("highlight_clipping_pct", 0.0))
            sh_pct = float(m.get("shadow_clipping_pct", 0.0))

            # Highlights are harder to recover if fully blown across all channels
            highlight_potential = max(0.0, 1.0 - (hi_pct / 15.0))
            shadow_potential = max(0.0, 1.0 - (sh_pct / 30.0))

        overall_recoverability = round((highlight_potential * 0.6) + (shadow_potential * 0.4), 4)

        return {
            "overall_recoverability": overall_recoverability,
            "highlight_recovery_potential": round(highlight_potential, 4),
            "shadow_recovery_potential": round(shadow_potential, 4),
            "assessment": "High RAW potential" if overall_recoverability > 0.75 else "Limited recovery potential",
        }
