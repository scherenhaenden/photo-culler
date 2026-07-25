"""Abstract Base Class for all Analyzers in the photo-culler framework."""

from abc import ABC, abstractmethod
from typing import Dict, Any
import time

from .context import AnalysisContext
from .result import AnalysisResult


class Analyzer(ABC):
    """Base interface for all photo analyzers.
    
    Every analyzer acts as an independent execution node in the pipeline,
    extracting specific measurements without making culling decisions.
    """
    
    name: str = "base_analyzer"
    version: str = "1.0"
    category: str = "general"
    enabled_by_default: bool = True

    def run(self, context: AnalysisContext) -> AnalysisResult:
        """Wrapper method that measures execution time and handles exceptions safely."""
        start_time = time.perf_counter()
        try:
            result = self.analyze(context)
            elapsed_ms = (time.perf_counter() - start_time) * 1000.0
            result.execution_time_ms = round(elapsed_ms, 3)
            return result
        except Exception as e:
            elapsed_ms = (time.perf_counter() - start_time) * 1000.0
            return AnalysisResult(
                analyzer=self.name,
                version=self.version,
                metrics={},
                confidence=0.0,
                error=str(e),
                execution_time_ms=round(elapsed_ms, 3),
            )

    @abstractmethod
    def analyze(self, context: AnalysisContext) -> AnalysisResult:
        """Perform analysis on the provided context and return AnalysisResult.
        
        Subclasses must implement this method.
        """
        pass
