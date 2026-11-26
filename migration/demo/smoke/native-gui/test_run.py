"""Focused regression tests for native GUI screenshot readiness checks."""

import binascii
import importlib.util
import struct
import tempfile
import unittest
import zlib
from pathlib import Path


RUNNER_PATH = Path(__file__).with_name("run.py")
SPEC = importlib.util.spec_from_file_location("native_gui_runner", RUNNER_PATH)
assert SPEC and SPEC.loader
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


def png_bytes(width, height, paint_yellow=True):
    rows = []
    for y in range(height):
        row = bytearray(width * 3)
        for x in range(450, 490):
            if 130 <= y < 170:
                offset = x * 3
                row[offset:offset + 3] = bytes((55, 130, 205))
        if paint_yellow and 140 <= y < 146:
            for x in range(462, 468):
                offset = x * 3
                row[offset:offset + 3] = bytes((255, 220, 70))
        # An orange UI control lies in the broad gallery scan region, but is
        # not a fixture yellow and must not contribute to readiness.
        if 220 <= y < 250:
            for x in range(800, 850):
                offset = x * 3
                row[offset:offset + 3] = bytes((255, 120, 40))
        rows.append(b"\x00" + row)

    def chunk(kind, payload):
        return (struct.pack(">I", len(payload)) + kind + payload
                + struct.pack(">I", binascii.crc32(kind + payload) & 0xFFFFFFFF))

    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header)
            + chunk(b"IDAT", zlib.compress(b"".join(rows))) + chunk(b"IEND", b""))


class NativeGuiReadinessTests(unittest.TestCase):
    def analyze(self, paint_yellow):
        with tempfile.TemporaryDirectory() as temporary:
            screenshot = Path(temporary) / "screenshot.png"
            screenshot.write_bytes(png_bytes(1200, 800, paint_yellow))
            return runner.gallery_fixture_pixels(screenshot)

    def test_fixture_colors_are_detected_despite_unrelated_orange_controls(self):
        result = self.analyze(paint_yellow=True)

        self.assertTrue(result["fixture_colors_visible"])
        self.assertEqual(result["yellow_sun_pixels"], 36)
        self.assertGreaterEqual(result["blue_sky_pixels"], 100)

    def test_blue_content_without_fixture_yellow_does_not_report_ready(self):
        result = self.analyze(paint_yellow=False)

        self.assertFalse(result["fixture_colors_visible"])
        self.assertEqual(result["yellow_sun_pixels"], 0)
        self.assertGreaterEqual(result["blue_sky_pixels"], 100)


if __name__ == "__main__":
    unittest.main()
