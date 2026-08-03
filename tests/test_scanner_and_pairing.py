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
    assert filter_inst.classify_role(Path("test.NRW")) == FileRole.RAW
    assert filter_inst.classify_role(Path("test.JPG")) == FileRole.JPEG
    assert filter_inst.classify_role(Path("test.PNG")) == FileRole.IMAGE
    assert filter_inst.classify_role(Path("test.HEIC")) == FileRole.IMAGE
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


def test_scanner_does_not_follow_file_symlinks_outside_source(tmp_path):
    source = tmp_path / "source"
    outside = tmp_path / "outside"
    source.mkdir()
    outside.mkdir()
    external_photo = outside / "external.jpg"
    external_photo.write_bytes(b"external")
    (source / "linked.jpg").symlink_to(external_photo)

    assert list(DirectoryScanner().scan(source)) == []


def test_scanner_applies_source_relative_exclusion_patterns(tmp_path):
    nested = tmp_path / "previews"
    nested.mkdir()
    (tmp_path / "keep.jpg").write_bytes(b"keep")
    (tmp_path / "skip.jpg").write_bytes(b"skip")
    (nested / "cached.jpg").write_bytes(b"cached")

    records = list(
        DirectoryScanner().scan(
            tmp_path,
            exclude_patterns=["skip*.jpg", "previews/**"],
        )
    )

    assert [record.path.name for record in records] == ["keep.jpg"]


def test_photo_is_tandem_property():
    from photo_culler.core.enums import FileRole
    from photo_culler.core.models import FileRecord, Photo

    # 1. Tandem case: has both RAW and JPEG
    tandem_photo = Photo(
        photo_id="tandem-test",
        stem_name="DSC_100",
        files=[
            FileRecord(Path("DSC_100.NEF"), FileRole.RAW, 1000, 1.0),
            FileRecord(Path("DSC_100.JPG"), FileRole.JPEG, 500, 1.0),
        ],
    )
    assert tandem_photo.is_tandem is True

    # 2. Only RAW case
    raw_only_photo = Photo(
        photo_id="raw-only",
        stem_name="DSC_101",
        files=[
            FileRecord(Path("DSC_101.NEF"), FileRole.RAW, 1000, 1.0),
        ],
    )
    assert raw_only_photo.is_tandem is False

    # 3. Only JPEG case
    jpeg_only_photo = Photo(
        photo_id="jpeg-only",
        stem_name="DSC_102",
        files=[
            FileRecord(Path("DSC_102.JPG"), FileRole.JPEG, 500, 1.0),
        ],
    )
    assert jpeg_only_photo.is_tandem is False
