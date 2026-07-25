"""Motion Blur Analyzer measuring directional streak vectors and anisotropic vs defocus blur."""

import numpy as np

from ...engine.analyzer import Analyzer
from ...engine.context import AnalysisContext
from ...engine.result import AnalysisResult


class MotionBlurAnalyzer(Analyzer):
    """Estimates motion blur direction angle, streak length, and directional blur confidence."""

    name = "motion_blur"
    version = "1.0"
    category = "technical"

    def analyze(self, context: AnalysisContext) -> AnalysisResult:
        arr = context.get_numpy_array()
        if arr.ndim == 3:
            gray = 0.2126 * arr[:, :, 0] + 0.7152 * arr[:, :, 1] + 0.0722 * arr[:, :, 2]
        else:
            gray = arr.astype(np.float32)

        gy, gx = np.gradient(gray)

        # Calculate 2D gradient structure tensor elements
        gxx_mean = float(np.mean(gx**2))
        gyy_mean = float(np.mean(gy**2))
        gxy_mean = float(np.mean(gx * gy))

        # Eigenvalue decomposition of structure tensor matrix [[gxx, gxy], [gxy, gyy]]
        trace = gxx_mean + gyy_mean
        det = gxx_mean * gyy_mean - gxy_mean**2
        lambda1 = max(0.0, (trace / 2.0) + np.sqrt(max(0.0, (trace / 2.0) ** 2 - det)))
        lambda2 = max(0.0, (trace / 2.0) - np.sqrt(max(0.0, (trace / 2.0) ** 2 - det)))

        # Anisotropy: high anisotropy indicates strong directional motion blur; low means isotropic/defocus
        anisotropy = float((lambda1 - lambda2) / (lambda1 + lambda2 + 1e-8))

        # Direction of dominant motion angle in degrees
        angle_rad = 0.5 * np.arctan2(2 * gxy_mean, gxx_mean - gyy_mean)
        blur_angle_deg = float(np.degrees(angle_rad) % 180.0)

        # Estimated blur streak length scale proportional to anisotropy and inverse gradient magnitude
        grad_mag_mean = np.sqrt(gxx_mean + gyy_mean)
        blur_length_px = float(anisotropy * 15.0 / (grad_mag_mean + 1e-3))

        metrics = {
            "blur_direction_deg": round(blur_angle_deg, 1),
            "blur_length_px": round(blur_length_px, 2),
            "directional_anisotropy": round(anisotropy, 4),
            "motion_blur_confidence": round(min(1.0, anisotropy * 1.5), 4),
            "is_directional_motion_blur": anisotropy > 0.45 and blur_length_px > 3.0,
        }

        return AnalysisResult(
            analyzer=self.name,
            version=self.version,
            metrics=metrics,
            confidence=0.88,
        )
