"""Unit tests for scanner and RAW/JPEG pairing."""

from pathlib import Path

from PIL import Image

from photo_culler.core.enums import FileRole
from photo_culler.pairing.raw_jpeg_pairer import RawJpegPairer
from photo_culler.scanner.directory_scanner import DirectoryScanner
from photo_culler.scanner.file_filter import FileFilter


def test_file_filter():
    filter_inst = FileFilter()
    assert filter_inst.classify_role(Path("test.NEF")) == FileRole.RAW
    assert filter_inst.classify_role(Path("test.JPG")) == FileRole.JPEG
    assert filter_inst.classify_role(Path("test.xmp")) == FileRole.SIDECAR
    assert filter_inst.classify_role(Path("test.txt")) is None


def test_scanner_and_pairing(tmp_path):
    # Create sample media files
    raw_path = tmp_path / "DSC_100.NEF"
    jpg_path = tmp_path / "DSC_100.JPG"
    xmp_path = tmp_path / "DSC_100.xmp"

    img = Image.new("RGB", (100, 100), color=(100, 100, 100))
    img.save(jpg_path)

    with open(raw_path, "wb") as f:
        f.write(b"RAW_HEADER_DATA" * 100)
    with open(xmp_path, "w") as f:
        f.write("<xmp>sidecar</xmp>")

    scanner = DirectoryScanner()
    file_records = list(scanner.scan(tmp_path))
    assert len(file_records) == 3

    pairer = RawJpegPairer()
    photos = pairer.pair_files(file_records)
    assert len(photos) == 1
    photo = photos[0]
    assert photo.stem_name == "DSC_100"
    assert len(photo.files) == 3
    assert photo.primary_file.role == FileRole.RAW
