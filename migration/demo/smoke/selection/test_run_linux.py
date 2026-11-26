"""Focused tests for the persistent-selection acceptance runner."""

import importlib.util
import sqlite3
import tempfile
import unittest
from unittest import mock
from pathlib import Path


RUNNER_PATH = Path(__file__).with_name("run_linux.py")
SPEC = importlib.util.spec_from_file_location("selection_runner", RUNNER_PATH)
assert SPEC and SPEC.loader
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


class SelectionRunnerTests(unittest.TestCase):
    def test_mutating_api_requests_send_exact_local_origin(self):
        class Response:
            def __enter__(self):
                return self

            def __exit__(self, *_args):
                return False

            def read(self):
                return b"{}"

        origin = "http://127.0.0.1:43127"
        with mock.patch.object(runner.urllib.request, "urlopen", return_value=Response()) as urlopen:
            runner.request(
                origin, "session-token", "/api/v2/import-estimates",
                {"source_id": "source-id", "recursive": True, "exclude_patterns": []},
            )

        request = urlopen.call_args.args[0]
        self.assertEqual(request.get_header("Origin"), origin)
        self.assertEqual(request.get_header("Authorization"), "Bearer session-token")

    def test_active_gallery_response_supplies_the_dynamic_id(self):
        self.assertEqual(
            runner.active_gallery_id({"contract_version": 1, "gallery_id": "generated-id"}),
            "generated-id",
        )

    def test_active_gallery_response_must_have_one_supported_gallery(self):
        for response in (
            {"contract_version": 1, "gallery_id": None},
            {"contract_version": 1, "gallery_id": ""},
            {"contract_version": 2, "gallery_id": "generated-id"},
        ):
            with self.subTest(response=response), self.assertRaisesRegex(
                RuntimeError, "did not report one active gallery"
            ):
                runner.active_gallery_id(response)

    def test_catalog_assertions_are_scoped_to_the_bootstrapped_gallery(self):
        with tempfile.TemporaryDirectory() as temporary:
            database = Path(temporary) / "catalog.sqlite3"
            with sqlite3.connect(database) as connection:
                connection.execute(
                    "CREATE TABLE photos(gallery_id TEXT, photo_id TEXT, "
                    "stem_name TEXT, decision TEXT)"
                )
                connection.executemany(
                    "INSERT INTO photos VALUES (?, ?, ?, ?)",
                    [
                        ("generated-id", f"photo-{index:02}", f"photo-{index:02}", "UNPROCESSED")
                        for index in range(1, 14)
                    ] + [("unrelated-gallery", "other", "other-photo", "KEEP")],
                )

            ids = runner.page_ids(database, "generated-id")

            self.assertEqual(len(ids), 13)
            self.assertEqual(runner.second_page_last_name(database, "generated-id"), "photo-13.jpg")
            display_names = runner.first_page_display_names(database, "generated-id")
            self.assertEqual(
                display_names,
                [f"photo-{index:02}.jpg" for index in range(12, 0, -1)],
            )
            geometry = {"x": 1, "y": 20, "width": 1440, "height": 900}
            self.assertEqual(
                runner.grid_cell_center(geometry, display_names.index("photo-01.jpg")),
                (639, 364),
            )
            self.assertEqual(
                runner.grid_cell_center(geometry, display_names.index("photo-02.jpg")),
                (521, 364),
            )
            self.assertEqual(runner.next_photo_button_center(geometry), (959, 123))
            runner.check_catalog(database, "generated-id", {"photo-01": "UNPROCESSED"})
            runner.check_counts(
                database, "generated-id", keep=0, reject=0, unrated=13, total=13
            )


if __name__ == "__main__":
    unittest.main()
