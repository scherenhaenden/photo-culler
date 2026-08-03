"""Opt-in parity test for the native pixel engine."""

from __future__ import annotations

import os

import numpy as np
import pytest
from PIL import Image

from photo_culler.analysis.shadow import RustShadowComparator


@pytest.mark.skipif(not os.environ.get("PHOTO_CULLER_RUST_CLI"), reason="native shadow CLI is not configured")
def test_native_shadow_matches_python_technical_metrics(tmp_path):
    pixels = np.zeros((48, 64, 3), dtype=np.uint8)
    pixels[:, 16:48] = (180, 120, 80)
    pixels[12:36, 24:40] = (255, 255, 255)
    source = tmp_path / "shadow-reference.png"
    Image.fromarray(pixels).save(source)

    comparison = RustShadowComparator().compare(source)

    assert comparison.is_within_tolerance(absolute=0.02, relative=0.02), comparison.deltas
