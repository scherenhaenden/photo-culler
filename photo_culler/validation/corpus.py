"""Validation Corpus & Ground Truth Benchmark Evaluator."""

from dataclasses import dataclass
from typing import Dict, List, Optional, Union

from photo_culler.core.enums import DecisionState


@dataclass
class GroundTruthLabel:
    photo_id: str
    expected_decision: Union[DecisionState, str]
    is_artistic_blur: bool = False
    is_recoverable_raw: bool = False
    notes: Optional[str] = None


class BenchmarkEvaluator:
    """Evaluates photo-culler culling decisions against a human gold-standard benchmark corpus."""

    def __init__(self, ground_truths: List[GroundTruthLabel]):
        self.ground_truths = {gt.photo_id: gt for gt in ground_truths}

    def evaluate(self, actual_decisions: Dict[str, Union[DecisionState, str]]) -> Dict[str, float]:
        """Compute F1-Score, Precision, Recall, False Rejection Rate (FRR), and False Acceptance Rate (FAR)."""
        tp = 0
        tn = 0
        fp = 0
        fn = 0

        keep_values = {
            DecisionState.KEEP,
            DecisionState.BEST,
            DecisionState.ALTERNATE,
            "keep",
            "best",
            "alternate",
            "KEEP",
            "BEST",
            "ALTERNATE",
        }

        for photo_id, gt in self.ground_truths.items():
            actual = actual_decisions.get(photo_id, DecisionState.REVIEW)
            expected = gt.expected_decision

            expected_is_keep = expected in keep_values
            actual_is_keep = actual in keep_values

            if expected_is_keep and actual_is_keep:
                tp += 1
            elif not expected_is_keep and not actual_is_keep:
                tn += 1
            elif not expected_is_keep and actual_is_keep:
                fp += 1
            elif expected_is_keep and not actual_is_keep:
                fn += 1

        total = max(1, tp + tn + fp + fn)
        precision = tp / max(1, tp + fp)
        recall = tp / max(1, tp + fn)
        f1_score = 2 * (precision * recall) / max(0.001, precision + recall)
        false_rejection_rate = fn / max(1, tp + fn)
        false_acceptance_rate = fp / max(1, tn + fp)

        return {
            "accuracy": round((tp + tn) / total, 4),
            "precision": round(precision, 4),
            "recall": round(recall, 4),
            "f1_score": round(f1_score, 4),
            "false_rejection_rate": round(false_rejection_rate, 4),
            "false_acceptance_rate": round(false_acceptance_rate, 4),
            "total_evaluated": total,
        }
