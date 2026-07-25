"""Technical analyzers package."""

from ...engine.registry import default_registry
from .clipping import ClippingAnalyzer
from .corruption import CorruptionAnalyzer
from .dimensions import DimensionsAnalyzer
from .exposure import ExposureAnalyzer
from .histogram import HistogramAnalyzer
from .motion_blur import MotionBlurAnalyzer
from .noise import NoiseAnalyzer
from .sharpness import SharpnessAnalyzer

# Register all technical analyzers in global registry
default_registry.register(CorruptionAnalyzer)
default_registry.register(DimensionsAnalyzer)
default_registry.register(HistogramAnalyzer)
default_registry.register(ClippingAnalyzer)
default_registry.register(ExposureAnalyzer)
default_registry.register(SharpnessAnalyzer)
default_registry.register(MotionBlurAnalyzer)
default_registry.register(NoiseAnalyzer)

__all__ = [
    "CorruptionAnalyzer",
    "DimensionsAnalyzer",
    "HistogramAnalyzer",
    "ClippingAnalyzer",
    "ExposureAnalyzer",
    "SharpnessAnalyzer",
    "MotionBlurAnalyzer",
    "NoiseAnalyzer",
]
