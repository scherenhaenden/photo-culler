"""Extended unit tests for metadata extraction."""

import pytest
from PIL import Image
from pathlib import Path
from photo_culler.metadata.extractor import MetadataExtractor


def test_metadata_extractor_fallback(tmp_path):
    img_path = tmp_path / "no_exif.jpg"
    img = Image.new("RGB", (100, 100), color=(50, 50, 50))
    img.save(img_path)

    extractor = MetadataExtractor()
    record = extractor.extract(img_path)

    assert record.camera_model is None
