"""Shadow comparison between the Python reference and the native pixel engine.

This module never changes a culling decision.  It is an operator tool used to
measure numerical parity before a Rust metric can be promoted.
"""

from __future__ import annotations

import json
import os
import subprocess
from dataclasses import dataclass
from pathlib import Path
from typing import Any, cast

from .analyzers.technical import ClippingAnalyzer, ExposureAnalyzer, HistogramAnalyzer, NoiseAnalyzer, SharpnessAnalyzer
from .engine.context import AnalysisContext


@dataclass(frozen=True)
class MetricDelta:
    """One comparable scalar emitted by the two technical engines."""

    name: str
    python: float
    rust: float

    @property
    def absolute_error(self) -> float:
        return abs(self.python - self.rust)

    @property
    def relative_error(self) -> float:
        return self.absolute_error / max(abs(self.python), 1e-9)


@dataclass(frozen=True)
class ShadowComparison:
    """Parity result for one source image; it is safe to persist or report."""

    source: Path
    deltas: tuple[MetricDelta, ...]

    def is_within_tolerance(self, absolute: float = 0.01, relative: float = 0.01) -> bool:
        return all(delta.absolute_error <= absolute or delta.relative_error <= relative for delta in self.deltas)


class RustShadowComparator:
    """Run the native CLI beside Python's technical analyzers without cutover."""

    def __init__(self, binary: str | Path | None = None, max_dimension: int = 1920):
        configured = binary or os.environ.get("PHOTO_CULLER_RUST_CLI")
        if configured is None:
            raise ValueError("Set PHOTO_CULLER_RUST_CLI to the built photo-culler-cli executable.")
        self.binary = Path(configured)
        self.max_dimension = max_dimension

    def compare(self, source: str | Path) -> ShadowComparison:
        source_path = Path(source).resolve()
        native = self._native_metrics(source_path)
        reference = self._python_metrics(source_path)
        deltas = tuple(
            MetricDelta(name=name, python=python_value, rust=float(native[native_name]))
            for name, native_name, python_value in reference
        )
        return ShadowComparison(source=source_path, deltas=deltas)

    def _native_metrics(self, source: Path) -> dict[str, Any]:
        if not self.binary.is_file() or not os.access(self.binary, os.X_OK):
            raise FileNotFoundError(f"Native shadow CLI is not an executable file: {self.binary}")
        if not source.is_file():
            raise FileNotFoundError(f"Image for native shadow comparison does not exist: {source}")

        completed = subprocess.run(
            [str(self.binary), "analyze", str(source), str(self.max_dimension)],
            check=True,
            capture_output=True,
            text=True,
            timeout=120,
            shell=False,
        )
        return cast(dict[str, Any], json.loads(completed.stdout))

    def _python_metrics(self, source: Path) -> list[tuple[str, str, float]]:
        context = AnalysisContext(source)
        try:
            histogram = HistogramAnalyzer().run(context).metrics
            clipping = ClippingAnalyzer().run(context).metrics
            exposure = ExposureAnalyzer().run(context).metrics
            noise = NoiseAnalyzer().run(context).metrics
            sharpness = SharpnessAnalyzer().run(context).metrics
        finally:
            context.close()
        return [
            ("histogram.mean_luminance", "mean_luminance", float(histogram["mean_luminance"])),
            ("histogram.std_luminance", "luminance_stddev", float(histogram["std_luminance"])),
            ("histogram.entropy", "luminance_entropy", float(histogram["entropy"])),
            ("exposure.score", "exposure_score", float(exposure["exposure_score"])),
            (
                "exposure.underexposed_probability",
                "underexposed_probability",
                float(exposure["underexposed_probability"]),
            ),
            ("exposure.overexposed_probability", "overexposed_probability", float(exposure["overexposed_probability"])),
            ("clipping.shadow_ratio", "shadow_clipping_ratio", float(clipping["shadow_clipping_pct"]) / 100),
            ("clipping.highlight_ratio", "highlight_clipping_ratio", float(clipping["highlight_clipping_pct"]) / 100),
            (
                "clipping.center_shadow_ratio",
                "center_shadow_clipping_ratio",
                float(clipping["center_shadow_clipping_pct"]) / 100,
            ),
            (
                "clipping.center_highlight_ratio",
                "center_highlight_clipping_ratio",
                float(clipping["center_highlight_clipping_pct"]) / 100,
            ),
            ("noise.luminance_stddev", "luminance_noise_stddev", float(noise["luminance_noise_std"])),
            ("noise.chroma_stddev", "chroma_noise_stddev", float(noise["chroma_noise_std"])),
            ("noise.shadow_stddev", "shadow_noise_stddev", float(noise["shadow_noise_std"])),
            ("noise.estimated_level", "estimated_noise_level", float(noise["estimated_noise_level"])),
            ("sharpness.global_score", "global_sharpness", float(sharpness["global_sharpness"])),
            ("sharpness.laplacian_variance", "laplacian_variance", float(sharpness["laplacian_variance"])),
            (
                "sharpness.center_laplacian_variance",
                "center_laplacian_variance",
                float(sharpness["center_laplacian_variance"]),
            ),
            (
                "sharpness.effective_focus_variance",
                "effective_focus_variance",
                float(sharpness["effective_focus_variance"]),
            ),
            ("sharpness.gradient_energy", "gradient_energy", float(sharpness["gradient_energy"])),
            ("sharpness.edge_density", "edge_density", float(sharpness["edge_density"])),
            ("sharpness.fft_high_frequency_ratio", "fft_high_frequency_ratio", float(sharpness["fft_high_freq_ratio"])),
        ]
