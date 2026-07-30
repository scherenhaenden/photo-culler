"""Unit tests for Tier 1 technical analyzers."""

import numpy as np
import pytest
from PIL import Image, ImageDraw

from photo_culler.analysis.analyzers.technical import (
    ClippingAnalyzer,
    CorruptionAnalyzer,
    DimensionsAnalyzer,
    ExposureAnalyzer,
    HistogramAnalyzer,
    MotionBlurAnalyzer,
    NoiseAnalyzer,
    SharpnessAnalyzer,
)
from photo_culler.analysis.engine.context import AnalysisContext


@pytest.fixture
def test_images(tmp_path):
    # 1. Healthy image with sharp contrast edges
    sharp_path = tmp_path / "sharp.jpg"
    img_sharp = Image.new("RGB", (400, 300), color=(50, 50, 50))
    draw = ImageDraw.Draw(img_sharp)
    draw.rectangle([50, 50, 350, 250], fill=(220, 220, 220), outline=(0, 0, 0), width=5)
    img_sharp.save(sharp_path)

    # 2. Corrupt / truncated image
    corrupt_path = tmp_path / "corrupt.jpg"
    with open(sharp_path, "rb") as f:
        data = f.read()
    with open(corrupt_path, "wb") as f:
        f.write(data[: len(data) // 4])  # Cut file short

    # 3. High noise synthetic image
    noise_path = tmp_path / "noisy.png"
    arr_noise = np.random.randint(0, 255, (200, 200, 3), dtype=np.uint8)
    Image.fromarray(arr_noise).save(noise_path)

    return {
        "sharp": sharp_path,
        "corrupt": corrupt_path,
        "noisy": noise_path,
    }


def test_corruption_analyzer(test_images):
    analyzer = CorruptionAnalyzer()

    # Healthy file
    ctx_healthy = AnalysisContext(test_images["sharp"])
    res_healthy = analyzer.run(ctx_healthy)
    assert res_healthy.metrics["corruption_status"] == "healthy"
    assert res_healthy.metrics["is_decodable"] is True
    ctx_healthy.close()

    # Corrupt file
    ctx_corrupt = AnalysisContext(test_images["corrupt"])
    res_corrupt = analyzer.run(ctx_corrupt)
    assert res_corrupt.metrics["corruption_status"] in ("corrupted", "probably_corrupted")
    assert res_corrupt.metrics["is_decodable"] is False
    ctx_corrupt.close()


def test_dimensions_analyzer(test_images):
    analyzer = DimensionsAnalyzer()
    ctx = AnalysisContext(test_images["sharp"])
    res = analyzer.run(ctx)
    assert res.metrics["width"] == 400
    assert res.metrics["height"] == 300
    assert res.metrics["orientation"] == "landscape"
    assert res.metrics["megapixels"] == 0.12
    ctx.close()


def test_histogram_and_clipping(test_images):
    hist_analyzer = HistogramAnalyzer()
    clip_analyzer = ClippingAnalyzer()

    ctx = AnalysisContext(test_images["sharp"])
    res_hist = hist_analyzer.run(ctx)
    assert "percentile_1" in res_hist.metrics
    assert "entropy" in res_hist.metrics
    assert "histogram" in ctx.shared_features

    res_clip = clip_analyzer.run(ctx)
    assert "highlight_clipping_pct" in res_clip.metrics
    assert "shadow_clipping_pct" in res_clip.metrics
    ctx.close()


def test_exposure_analyzer(test_images):
    exposure_analyzer = ExposureAnalyzer()
    ctx = AnalysisContext(test_images["sharp"])
    res = exposure_analyzer.run(ctx)
    assert "exposure_score" in res.metrics
    assert 0.0 <= res.metrics["exposure_score"] <= 1.0
    ctx.close()


def test_sharpness_and_motion_blur(test_images):
    sharp_analyzer = SharpnessAnalyzer()
    motion_analyzer = MotionBlurAnalyzer()

    ctx = AnalysisContext(test_images["sharp"])
    res_sharp = sharp_analyzer.run(ctx)
    assert "global_sharpness" in res_sharp.metrics
    assert "laplacian_variance" in res_sharp.metrics
    assert res_sharp.metrics["laplacian_variance"] > 0

    res_motion = motion_analyzer.run(ctx)
    assert "blur_direction_deg" in res_motion.metrics
    assert "directional_anisotropy" in res_motion.metrics
    ctx.close()


def test_noise_analyzer(test_images):
    noise_analyzer = NoiseAnalyzer()

    # Test on noisy image
    ctx_noisy = AnalysisContext(test_images["noisy"])
    res_noisy = noise_analyzer.run(ctx_noisy)
    assert res_noisy.metrics["estimated_noise_level"] > 0.3
    ctx_noisy.close()
