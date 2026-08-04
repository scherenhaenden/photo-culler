"""Geometry analyzers used by non-destructive edit suggestions."""

from ...engine.registry import default_registry
from .horizon import HorizonAnalyzer

default_registry.register(HorizonAnalyzer)

__all__ = ["HorizonAnalyzer"]
