"""AnalysisResult data structure for the photo-culler analysis framework."""

import json
from dataclasses import asdict, dataclass, field
from typing import Any, Dict, Optional


@dataclass
class AnalysisResult:
    """Standard output produced by every analyzer.

    Attributes:
        analyzer: Unique name identifier of the analyzer.
        version: Version string of the analyzer implementation.
        metrics: Dictionary of numerical, boolean, or categorical measurements.
        confidence: Float score from 0.0 to 1.0 representing analyzer confidence.
        error: Optional error string if analysis encountered a non-fatal failure.
        execution_time_ms: Wall-clock execution duration in milliseconds.
    """

    analyzer: str
    version: str
    metrics: Dict[str, Any] = field(default_factory=dict)
    confidence: float = 1.0
    error: Optional[str] = None
    execution_time_ms: float = 0.0

    def to_dict(self) -> Dict[str, Any]:
        """Convert result to a standard serializable dictionary."""
        return asdict(self)

    def to_json(self) -> str:
        """Convert result to JSON string."""
        return json.dumps(self.to_dict(), indent=2)

    @classmethod
    def from_dict(cls, data: Dict[str, Any]) -> "AnalysisResult":
        """Reconstruct AnalysisResult from dictionary."""
        return cls(
            analyzer=data["analyzer"],
            version=data.get("version", "1.0"),
            metrics=data.get("metrics", {}),
            confidence=float(data.get("confidence", 1.0)),
            error=data.get("error"),
            execution_time_ms=float(data.get("execution_time_ms", 0.0)),
        )
