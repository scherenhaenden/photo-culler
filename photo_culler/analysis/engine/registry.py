"""Registry for managing and discovering analyzers."""

from typing import Dict, Type, List, Optional
from .analyzer import Analyzer


class AnalyzerRegistry:
    """Registry pattern for registering, discovering, and instantiating analyzers."""

    def __init__(self):
        self._registry: Dict[str, Type[Analyzer]] = {}

    def register(self, analyzer_cls: Type[Analyzer]):
        """Register an Analyzer subclass."""
        if not issubclass(analyzer_cls, Analyzer):
            raise TypeError(f"Class {analyzer_cls} must inherit from Analyzer")
        
        name = analyzer_cls.name
        self._registry[name] = analyzer_cls
        return analyzer_cls

    def get(self, name: str) -> Optional[Type[Analyzer]]:
        """Retrieve analyzer class by name."""
        return self._registry.get(name)

    def list_analyzers(self, category: Optional[str] = None, enabled_only: bool = False) -> List[Type[Analyzer]]:
        """List registered analyzers, optionally filtered by category or default enabled state."""
        results = []
        for cls in self._registry.values():
            if category and cls.category != category:
                continue
            if enabled_only and not cls.enabled_by_default:
                continue
            results.append(cls)
        return results

    def instantiate_all(self, enabled_only: bool = True) -> List[Analyzer]:
        """Instantiate registered analyzer classes."""
        classes = self.list_analyzers(enabled_only=enabled_only)
        return [cls() for cls in classes]


# Global default registry instance
default_registry = AnalyzerRegistry()
