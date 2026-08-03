"""Unit tests for thumbnail preview generator."""

import sys
from types import SimpleNamespace

import numpy as np
from PIL import Image

from photo_culler.previews.generator import PreviewGenerator


def test_preview_generator(tmp_path):
    cache_dir = tmp_path / "cache"
    generator = PreviewGenerator(cache_dir=cache_dir)

    img_path = tmp_path / "test_img.jpg"
    img = Image.new("RGB", (2000, 1500), color=(100, 150, 200))
    img.save(img_path)

    results = generator.generate_thumbnails(photo_id="photo_123", image_path=img_path)

    assert "small" in results
    assert "medium" in results
    assert "large" in results
    assert "full" in results
    assert results["small"].exists()


def test_preview_generator_decodes_camera_raw_with_rawpy(tmp_path, monkeypatch):
    class FakeRaw:
        def __enter__(self):
            return self

        def __exit__(self, *args):
            return False

        def postprocess(self, **kwargs):
            assert kwargs == {"use_camera_wb": True, "output_bps": 8, "half_size": True}
            return np.full((12, 18, 3), (100, 150, 200), dtype=np.uint8)

    raw_path = tmp_path / "camera.nef"
    raw_path.write_bytes(b"camera raw data")
    monkeypatch.setitem(sys.modules, "rawpy", SimpleNamespace(imread=lambda path: FakeRaw()))

    results = PreviewGenerator(cache_dir=tmp_path / "cache").generate_thumbnails("raw-photo", raw_path)

    assert results["small"].exists()
    with Image.open(results["small"]) as preview:
        assert preview.size == (18, 12)
