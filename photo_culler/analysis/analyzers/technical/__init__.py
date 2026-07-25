"""Technical analyzers package."""

from .corruption import CorruptionAnalyzer
from .dimensions import DimensionsAnalyzer
from .histogram import HistogramAnalyzer
from .clipping import ClippingAnalyzer
from .exposure import ExposureAnalyzer
from .sharpness import SharpnessAnalyzer
from .motion_blur import MotionBlurAnalyzer
from .noise import NoiseAnalyzer

from ...engine.registry import default_registry

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
