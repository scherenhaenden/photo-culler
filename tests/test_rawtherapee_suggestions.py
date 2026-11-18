"""Tests for the non-destructive, explainable RawTherapee experiment."""

from photo_culler.analysis.engine.result import AnalysisResult
from photo_culler.restoration.rawtherapee import RawTherapeeProfileSuggester


def test_suggestion_turns_complete_cull_metrics_into_conservative_pp3():
    results = {
        "exposure": AnalysisResult("exposure", "1", {"mean_luminance": 50, "is_underexposed": True}),
        "clipping": {"highlight_clipping_pct": 4.0, "center_highlight_clipping_pct": 6.0, "shadow_clipping_pct": 10.0},
        "noise": {"estimated_noise_level": 0.42},
        "sharpness": {"is_soft": False},
        "motion_blur": {"is_directional_motion_blur": False},
        "horizon": {"has_reliable_horizon": True, "recommended_rotation_degrees": -1.5},
        "visual_balance": {
            "crop_recommended": True,
            "suggested_crop_normalized": {"x": 0.15, "y": 0.0, "width": 0.85, "height": 0.85},
        },
    }

    suggestion = RawTherapeeProfileSuggester().suggest(results)

    assert "[Exposure]" in suggestion.pp3
    assert "Compensation=0.91" in suggestion.pp3
    assert "HighlightCompr=53" in suggestion.pp3
    assert "ShadowCompr=40" in suggestion.pp3
    assert "[Directional Pyramid Denoising]" in suggestion.pp3
    assert "Enabled=true" in suggestion.pp3
    assert "[Sharpening]\nEnabled=false" in suggestion.pp3
    assert suggestion.confidence == 1.0
    assert suggestion.geometry["rotation_degrees"] == -1.5
    assert suggestion.geometry["crop_normalized"]["x"] == 0.15
    assert any("ruido" in reason.lower() for reason in suggestion.reasons)


def test_motion_blur_disables_sharpening_and_incomplete_analysis_is_flagged():
    suggestion = RawTherapeeProfileSuggester().suggest(
        {"motion_blur": {"is_directional_motion_blur": True}}
    )

    assert "[Sharpening]\nEnabled=false" in suggestion.pp3
    assert round(suggestion.confidence, 2) == 0.14
    assert any("blur direccional" in warning.lower() for warning in suggestion.warnings)
    assert any("Faltan métricas" in warning for warning in suggestion.warnings)
