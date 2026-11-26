"""Focused tests for the Sprint 02 packaged acceptance evidence checks."""

import importlib.util
import sqlite3
import tempfile
import unittest
from pathlib import Path


RUNNER_PATH = Path(__file__).with_name("run_s02_linux.py")
SPEC = importlib.util.spec_from_file_location("run_s02_linux", RUNNER_PATH)
assert SPEC and SPEC.loader
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


class Sprint02RunnerTests(unittest.TestCase):
    def test_catalog_snapshot_reads_ids_and_full_hashes_for_jpegs(self):
        with tempfile.TemporaryDirectory() as temporary:
            database = Path(temporary) / "catalog.sqlite3"
            with sqlite3.connect(database) as connection:
                connection.executescript(
                    "CREATE TABLE photos(id INTEGER PRIMARY KEY, photo_id TEXT, stem_name TEXT);"
                    "CREATE TABLE files(photo_id INTEGER, source_relative_path TEXT, full_hash TEXT, role TEXT);"
                    "INSERT INTO photos VALUES (1, 'stable-id-1', 'photo-01');"
                    "INSERT INTO files VALUES (1, 'photo-01.jpg', 'a' , 'JPEG');"
                    "INSERT INTO photos VALUES (2, 'stable-id-2', 'photo-02');"
                    "INSERT INTO files VALUES (2, 'photo-02.jpg', 'b', 'JPEG');"
                    "INSERT INTO files VALUES (2, 'photo-02.xmp', 'c', 'SIDECAR');"
                )

            count, rows = runner.database_snapshot(database)

            self.assertEqual(count, 2)
            self.assertEqual([row[1] for row in rows], ["stable-id-1", "stable-id-2"])
            runner.assert_catalog_hashes(rows, {"photo-01.jpg": "a", "photo-02.jpg": "b"})

    def test_catalog_hash_check_rejects_missing_or_changed_jpegs(self):
        rows = [("photo-01", "stable-id-1", "photo-01.jpg", "wrong")]
        with self.assertRaisesRegex(RuntimeError, "catalog full hashes differ"):
            runner.assert_catalog_hashes(rows, {"photo-01.jpg": "expected"})


if __name__ == "__main__":
    unittest.main()
