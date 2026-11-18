"""Regression coverage for conservative level/crop visual measurements."""

import numpy as np
from PIL import Image

from photo_culler.analysis.analyzers.composition.visual_balance import VisualBalanceAnalyzer
from photo_culler.analysis.analyzers.geometry.horizon import HorizonAnalyzer
from photo_culler.analysis.engine.context import AnalysisContext


def test_horizon_analyzer_suggests_opposite_rotation_for_a_sloping_boundary(tmp_path):
    path = tmp_path / "sloping-horizon.png"
    height, width = 180, 280
    image = np.zeros((height, width, 3), dtype=np.uint8)
    for x in range(width):
        image[int(70 + x * 0.08) :, x] = 230
    Image.fromarray(image).save(path)

    context = AnalysisContext(path)
    result = HorizonAnalyzer().run(context)
    context.close()

    assert result.metrics["has_reliable_horizon"] is True
    assert result.metrics["recommended_rotation_degrees"] < -1.0


def test_visual_balance_proposes_a_small_crop_for_off_center_visual_weight(tmp_path):
    path = tmp_path / "off-center.png"
    image = np.full((180, 280, 3), 80, dtype=np.uint8)
    image[50:130, 210:270] = 230
    Image.fromarray(image).save(path)

    context = AnalysisContext(path)
    result = VisualBalanceAnalyzer().run(context)
    context.close()

    assert result.metrics["crop_recommended"] is True
    assert result.metrics["suggested_crop_normalized"]["x"] > 0
