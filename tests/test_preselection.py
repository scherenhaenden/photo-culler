import os

from PIL import Image, ImageDraw

from photo_culler.cli.commands.preselect_cmd import _matching_raw
from photo_culler.selection.preselection import JpegPreselector


def _photo(path, colour, offset=0):
    image = Image.new("RGB", (160, 120), colour)
    draw = ImageDraw.Draw(image)
    draw.rectangle((30 + offset, 25, 90 + offset, 95), fill=(240, 180, 140))
    image.save(path)


def test_preselection_keeps_one_from_a_near_identical_burst_and_a_distinct_scene(tmp_path):
    first = tmp_path / "DSC_0001.jpg"
    second = tmp_path / "DSC_0002.jpg"
    distinct = tmp_path / "DSC_0003.jpg"
    _photo(first, (30, 70, 110))
    _photo(second, (31, 70, 110), offset=1)
    _photo(distinct, (170, 35, 20), offset=40)
    selector = JpegPreselector(max_gap_seconds=60, duplicate_similarity=0.88)
    selections = selector.select([first, second, distinct])
    assert len(selections) == 3
    assert sum(item.selected for item in selections) == 2
    assert selections[0].group_id == selections[1].group_id
    assert selections[2].group_id != selections[1].group_id


def test_preselection_does_not_group_lookalikes_from_distant_moments(tmp_path):
    first = tmp_path / "DSC_0001.jpg"
    second = tmp_path / "DSC_0100.jpg"
    _photo(first, (30, 70, 110))
    _photo(second, (30, 70, 110))
    os.utime(first, (1_700_000_000, 1_700_000_000))
    os.utime(second, (1_700_000_100, 1_700_000_100))
    selector = JpegPreselector(max_gap_seconds=0.1)
    left = selector.describe(first)
    right = selector.describe(second)
    assert selector._nearby(left, right) is False


def test_matching_raw_uses_same_stem_and_does_not_invent_missing_pairs(tmp_path):
    raw = tmp_path / "DSC_0001.NEF"
    raw.touch()
    assert _matching_raw(tmp_path, "DSC_0001") == str(raw)
    assert _matching_raw(tmp_path, "DSC_0002") == ""
