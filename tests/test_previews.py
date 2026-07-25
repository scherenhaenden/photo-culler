"""Unit tests for thumbnail preview generator."""

import pytest
from PIL import Image
from pathlib import Path
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
