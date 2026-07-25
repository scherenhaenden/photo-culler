"""Perceptual hashing (dHash) for visual similarity matching."""

from pathlib import Path
from typing import Optional

from PIL import Image


def compute_dhash(image_input, hash_size: int = 8) -> Optional[str]:
    """Compute difference hash (dHash) as a 16-character hexadecimal string.

    Args:
        image_input: Path or PIL.Image instance.
        hash_size: Grid resolution (default 8 yields 64-bit hash).
    """
    try:
        if isinstance(image_input, (str, Path)):
            img = Image.open(image_input)
        else:
            img = image_input

        # Resize to (width = hash_size + 1, height = hash_size) in grayscale
        img_resized = img.convert("L").resize((hash_size + 1, hash_size), Image.Resampling.BILINEAR)
        pixels = list(img_resized.getdata())

        # Compare adjacent horizontal pixels
        difference = []
        for row in range(hash_size):
            for col in range(hash_size):
                pixel_left = pixels[row * (hash_size + 1) + col]
                pixel_right = pixels[row * (hash_size + 1) + col + 1]
                difference.append(pixel_left > pixel_right)

        # Convert boolean array to hex string
        decimal_val = 0
        for i, val in enumerate(difference):
            if val:
                decimal_val |= 1 << i

        hex_str = f"{decimal_val:0{hash_size * 2 // 4}x}"
        return hex_str
    except Exception:
        return None


def hamming_distance(hash1: str, hash2: str) -> int:
    """Calculate bitwise Hamming distance between two hex perceptual hashes."""
    if not hash1 or not hash2 or len(hash1) != len(hash2):
        return 999
    val1 = int(hash1, 16)
    val2 = int(hash2, 16)
    return bin(val1 ^ val2).count("1")
