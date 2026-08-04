"""Composition analyzers used by non-destructive edit suggestions."""

from ...engine.registry import default_registry
from .visual_balance import VisualBalanceAnalyzer

default_registry.register(VisualBalanceAnalyzer)

__all__ = ["VisualBalanceAnalyzer"]
