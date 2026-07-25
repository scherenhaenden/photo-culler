"""Dimensions Analyzer for extracting resolution, aspect ratio, orientation, and megapixels."""

from ...engine.analyzer import Analyzer
from ...engine.context import AnalysisContext
from ...engine.result import AnalysisResult


class DimensionsAnalyzer(Analyzer):
    """Measures spatial dimensions, orientation classification, and pixel count."""

    name = "dimensions"
    version = "1.0"
    category = "technical"

    def analyze(self, context: AnalysisContext) -> AnalysisResult:
        pil_img = context.get_pillow_image()
        w, h = pil_img.size
        megapixels = round((w * h) / 1e6, 3)
        aspect_ratio = round(w / h, 4) if h > 0 else 0.0

        is_portrait = h > w
        is_landscape = w > h
        is_square = abs(w - h) / max(w, h) < 0.02 if max(w, h) > 0 else False
        is_panorama = aspect_ratio >= 2.0 or (h > 0 and (h / w) >= 2.0)

        orientation = "square"
        if is_panorama:
            orientation = "panorama"
        elif is_portrait:
            orientation = "portrait"
        elif is_landscape:
            orientation = "landscape"

        metrics = {
            "width": w,
            "height": h,
            "aspect_ratio": aspect_ratio,
            "megapixels": megapixels,
            "orientation": orientation,
            "is_portrait": is_portrait,
            "is_landscape": is_landscape,
            "is_square": is_square,
            "is_panorama": is_panorama,
        }

        return AnalysisResult(
            analyzer=self.name,
            version=self.version,
            metrics=metrics,
            confidence=1.0,
        )
