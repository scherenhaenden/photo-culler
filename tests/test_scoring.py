"""Unit tests for scoring modules with profile context and confidence evaluation."""

from photo_culler.analysis.engine.result import AnalysisResult
from photo_culler.scoring.recoverability_score import RecoverabilityScorer
from photo_culler.scoring.technical_score import TechnicalScorer


def test_technical_scorer_healthy():
    results = {
        "corruption": AnalysisResult("corruption", "1.0", {"corruption_status": "healthy"}, confidence=0.98),
        "sharpness": AnalysisResult("sharpness", "1.1", {"global_sharpness": 0.85}, confidence=0.92),
        "exposure": AnalysisResult("exposure", "1.0", {"exposure_score": 0.90}, confidence=0.95),
        "clipping": AnalysisResult(
            "clipping",
            "1.1",
            {
                "highlight_clipping_pct": 0.5,
                "shadow_clipping_pct": 1.0,
                "center_highlight_clipping_pct": 0.0,
            },
            confidence=1.0,
        ),
        "noise": AnalysisResult("noise", "1.0", {"estimated_noise_level": 0.05}, confidence=0.89),
    }

    scorer = TechnicalScorer(profile="concert")
    score_data = scorer.calculate_score(results)
    assert score_data["quality_tier"] == "excellent"
    assert score_data["final_score"] >= 0.80
    assert score_data["overall_confidence"] > 0.90
    assert score_data["profile_used"] == "concert"


def test_technical_scorer_corrupted():
    results = {
        "corruption": AnalysisResult("corruption", "1.0", {"corruption_status": "corrupted"}),
    }

    scorer = TechnicalScorer()
    score_data = scorer.calculate_score(results)
    assert score_data["final_score"] == 0.0
    assert score_data["quality_tier"] == "corrupted"


def test_recoverability_scorer():
    results = {
        "clipping": AnalysisResult("clipping", "1.1", {"highlight_clipping_pct": 2.0, "shadow_clipping_pct": 5.0}),
    }

    scorer = RecoverabilityScorer()
    rec_data = scorer.calculate_recoverability(results)
    assert 0.0 <= rec_data["overall_recoverability"] <= 1.0
    assert rec_data["overall_recoverability"] > 0.70
