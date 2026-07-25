"""Sharpness Analyzer combining Laplacian variance, edge density, gradient energy, and FFT high-frequency ratio."""

import numpy as np
from ...engine.analyzer import Analyzer
from ...engine.context import AnalysisContext
from ...engine.result import AnalysisResult


class SharpnessAnalyzer(Analyzer):
    """Measures image focus and edge definition via Laplacian variance, gradient energy, and FFT analysis."""

    name = "sharpness"
    version = "1.0"
    category = "technical"

    def analyze(self, context: AnalysisContext) -> AnalysisResult:
        arr = context.get_numpy_array()
        
        # Convert to grayscale
        if arr.ndim == 3:
            gray = 0.2126 * arr[:, :, 0] + 0.7152 * arr[:, :, 1] + 0.0722 * arr[:, :, 2]
        else:
            gray = arr.astype(np.float32)

        # 1. Laplacian Variance
        # Discrete Laplacian kernel: [[0, 1, 0], [1, -4, 1], [0, 1, 0]]
        gy, gx = np.gradient(gray)
        gyy, _ = np.gradient(gy)
        _, gxx = np.gradient(gx)
        laplacian = gxx + gyy
        laplacian_var = float(np.var(laplacian))

        # 2. Gradient Energy (Tenengrad criterion: mean squared gradient magnitude)
        grad_energy = float(np.mean(gx**2 + gy**2))

        # 3. Edge Density (proportion of strong gradient magnitude pixels)
        grad_mag = np.sqrt(gx**2 + gy**2)
        edge_threshold = 12.0
        edge_density = float(np.mean(grad_mag > edge_threshold))

        # 4. FFT High Frequency Ratio
        h, w = gray.shape
        cy, cx = h // 2, w // 2
        f = np.fft.fft2(gray)
        fshift = np.fft.fftshift(f)
        magnitude_spectrum = np.abs(fshift)
        
        # Mask out center low frequencies (radius = min(h,w)/10)
        r = min(h, w) // 10
        y_grid, x_grid = np.ogrid[:h, :w]
        mask = ((y_grid - cy)**2 + (x_grid - cx)**2) > r**2
        
        high_freq_power = float(np.sum(magnitude_spectrum[mask]))
        total_power = float(np.sum(magnitude_spectrum)) + 1e-8
        fft_ratio = float(high_freq_power / total_power)

        # Normalized global sharpness score (0.0 to 1.0 bounded logarithmic scaling)
        # Typical laplacian variance ranges from 10 (blurry) to 1000+ (sharp)
        global_sharpness = min(1.0, max(0.0, np.log10(max(1.0, laplacian_var)) / 3.5))

        metrics = {
            "global_sharpness": round(float(global_sharpness), 4),
            "laplacian_variance": round(laplacian_var, 2),
            "gradient_energy": round(grad_energy, 2),
            "edge_density": round(edge_density, 4),
            "fft_high_freq_ratio": round(fft_ratio, 4),
            "is_tack_sharp": laplacian_var > 300.0,
            "is_soft": laplacian_var < 50.0,
        }

        return AnalysisResult(
            analyzer=self.name,
            version=self.version,
            metrics=metrics,
            confidence=0.92,
        )
