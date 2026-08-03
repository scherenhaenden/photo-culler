"""Unit tests for thumbnail preview generator."""

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


def test_extract_embedded_jpeg_helper(tmp_path):
    from photo_culler.previews.generator import extract_embedded_jpeg
    import io

    # Create dummy raw file with embedded JPEG inside
    raw_path = tmp_path / "mock.nef"

    # Generate a real JPEG to embed (large enough to exceed 5000 bytes)
    jpeg_io = io.BytesIO()
    Image.new("RGB", (600, 600), color="blue").save(jpeg_io, format="JPEG")
    jpeg_bytes = jpeg_io.getvalue()
    assert len(jpeg_bytes) > 5000

    # Surround the JPEG bytes with dummy RAW wrapper bytes
    raw_data = b"RANDOM_RAW_HEADER_INFO" * 500 + jpeg_bytes + b"RANDOM_RAW_FOOTER_INFO" * 500
    raw_path.write_bytes(raw_data)

    extracted = extract_embedded_jpeg(raw_path)
    assert extracted is not None
    assert b"\xff\xd8\xff" in extracted
    assert b"\xff\xd9" in extracted

    # Test generation with standalone raw
    generator = PreviewGenerator(cache_dir=tmp_path / "cache_raw")
    results = generator.generate_thumbnails(photo_id="raw_photo", image_path=raw_path)
    assert "medium" in results
    assert results["medium"].exists()
