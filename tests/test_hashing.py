"""Unit tests for hashing algorithms."""

import pytest
from PIL import Image
from pathlib import Path

from photo_culler.identity.quick_hash import compute_quick_hash
from photo_culler.identity.full_hash import compute_full_hash
from photo_culler.identity.perceptual_hash import compute_dhash, hamming_distance


def test_quick_and_full_hash(tmp_path):
    test_file = tmp_path / "sample.bin"
    with open(test_file, "wb") as f:
        f.write(b"ABCDEF1234567890" * 1000)

    qhash = compute_quick_hash(test_file)
    fhash = compute_full_hash(test_file)
    assert len(qhash) == 64
    assert len(fhash) == 64
    assert qhash != ""
    assert fhash != ""


def test_perceptual_dhash(tmp_path):
    img_path1 = tmp_path / "img1.png"
    img_path2 = tmp_path / "img2.png"

    img1 = Image.new("RGB", (100, 100), color=(50, 50, 50))
    img2 = Image.new("RGB", (100, 100), color=(50, 50, 50))
    img1.save(img_path1)
    img2.save(img_path2)

    hash1 = compute_dhash(img_path1)
    hash2 = compute_dhash(img_path2)

    assert hash1 is not None
    assert hash2 is not None
    assert hamming_distance(hash1, hash2) == 0
