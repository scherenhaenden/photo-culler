"""Unit tests for the Sprint 03 deterministic photo fixture."""

import json
import tempfile
import unittest
from pathlib import Path

from PIL import Image

from fixture import write_fixture


class SelectionFixtureTests(unittest.TestCase):
    def test_creates_13_unique_jpegs_with_ordered_ids_and_manifest_hashes(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "fixture"
            manifest = write_fixture(root)

            self.assertEqual(manifest["photo_count"], 13)
            self.assertEqual(manifest["photos_per_page"], 12)
            self.assertTrue((root / "Carpeta vacía").is_dir())
            photos = manifest["photos"]
            self.assertEqual(
                [photo["photo_id"] for photo in photos],
                [f"photo-{number:02}" for number in range(1, 14)],
            )

            hashes = []
            for photo in photos:
                path = root / "Fotos de selección" / photo["filename"]
                self.assertEqual(path.stat().st_size, photo["size_bytes"])
                hashes.append(photo["sha256"])
                with Image.open(path) as image:
                    self.assertEqual(image.format, "JPEG")
                    self.assertEqual(image.size, (640, 480))
                    image.verify()

            self.assertEqual(len(set(hashes)), 13)
            saved_manifest = json.loads((root / "manifest.json").read_text())
            self.assertEqual(saved_manifest, manifest)

    def test_refuses_to_overwrite_an_existing_fixture(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary) / "fixture"
            write_fixture(root)
            with self.assertRaises(FileExistsError):
                write_fixture(root)


if __name__ == "__main__":
    unittest.main()
