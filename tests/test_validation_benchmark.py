"""Unit tests for BenchmarkEvaluator and GroundTruthLabel validation."""

from photo_culler.core.enums import DecisionState
from photo_culler.validation.corpus import BenchmarkEvaluator, GroundTruthLabel


def test_benchmark_evaluator_metrics():
    ground_truths = [
        GroundTruthLabel("photo_1", DecisionState.KEEP),
        GroundTruthLabel("photo_2", DecisionState.BEST),
        GroundTruthLabel("photo_3", DecisionState.REJECT_TECHNICAL),
        GroundTruthLabel("photo_4", DecisionState.REJECT_REDUNDANT),
    ]

    actual_decisions = {
        "photo_1": DecisionState.KEEP,
        "photo_2": DecisionState.BEST,
        "photo_3": DecisionState.REJECT_TECHNICAL,
        "photo_4": DecisionState.REJECT_REDUNDANT,
    }

    evaluator = BenchmarkEvaluator(ground_truths)
    results = evaluator.evaluate(actual_decisions)

    assert results["accuracy"] == 1.0
    assert results["precision"] == 1.0
    assert results["recall"] == 1.0
    assert results["f1_score"] == 1.0
    assert results["false_rejection_rate"] == 0.0
    assert results["false_acceptance_rate"] == 0.0
