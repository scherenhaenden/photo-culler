"""Sharpness Analyzer combining resolution-normalized global, regional, and center focus assessment."""

import numpy as np

from ...engine.analyzer import Analyzer
from ...engine.context import AnalysisContext
from ...engine.result import AnalysisResult


class SharpnessAnalyzer(Analyzer):
    """Measures image focus and edge definition via resolution-normalized Laplacian variance and regional analysis."""

    name = "sharpness"
    version = "1.1"
    category = "technical"

    def analyze(self, context: AnalysisContext) -> AnalysisResult:
        # Use resolution-normalized array to ensure scale consistency across sensor resolutions
        arr = context.get_analysis_array(max_dim=1920)

        # Convert to grayscale
        if arr.ndim == 3:
            gray = 0.2126 * arr[:, :, 0] + 0.7152 * arr[:, :, 1] + 0.0722 * arr[:, :, 2]
        else:
            gray = arr.astype(np.float32)

        h, w = gray.shape

        # 1. Global Laplacian Variance
        gy, gx = np.gradient(gray)
        gyy, _ = np.gradient(gy)
        _, gxx = np.gradient(gx)
        laplacian = gxx + gyy
        laplacian_var = float(np.var(laplacian))

        # 2. Central Region Sharpness (Middle 50% ROI where primary subject usually sits)
        cy_start, cy_end = int(h * 0.25), int(h * 0.75)
        cx_start, cx_end = int(w * 0.25), int(w * 0.75)
        center_laplacian = laplacian[cy_start:cy_end, cx_start:cx_end]
        center_laplacian_var = float(np.var(center_laplacian))

        # 3. Gradient Energy (Tenengrad criterion)
        grad_energy = float(np.mean(gx**2 + gy**2))

        # 4. Edge Density
        grad_mag = np.sqrt(gx**2 + gy**2)
        edge_threshold = 12.0
        edge_density = float(np.mean(grad_mag > edge_threshold))

        # 5. FFT High Frequency Ratio
        cy, cx = h // 2, w // 2
        f = np.fft.fft2(gray)
        fshift = np.fft.fftshift(f)
        magnitude_spectrum = np.abs(fshift)

        r = min(h, w) // 10
        y_grid, x_grid = np.ogrid[:h, :w]
        mask = ((y_grid - cy) ** 2 + (x_grid - cx) ** 2) > r**2

        high_freq_power = float(np.sum(magnitude_spectrum[mask]))
        total_power = float(np.sum(magnitude_spectrum)) + 1e-8
        fft_ratio = float(high_freq_power / total_power)

        # Weighted sharpness combining center subject focus (60%) and global focus (40%)
        effective_focus_var = (center_laplacian_var * 0.6) + (laplacian_var * 0.4)
        global_sharpness = min(1.0, max(0.0, np.log10(max(1.0, effective_focus_var)) / 3.5))

        metrics = {
            "global_sharpness": round(float(global_sharpness), 4),
            "laplacian_variance": round(laplacian_var, 2),
            "center_laplacian_variance": round(center_laplacian_var, 2),
            "effective_focus_variance": round(effective_focus_var, 2),
            "gradient_energy": round(grad_energy, 2),
            "edge_density": round(edge_density, 4),
            "fft_high_freq_ratio": round(fft_ratio, 4),
            "is_tack_sharp": effective_focus_var > 250.0,
            "is_soft": effective_focus_var < 45.0,
        }

        return AnalysisResult(
            analyzer=self.name,
            version=self.version,
            metrics=metrics,
            confidence=0.92,
        )
