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
            self.assertEqual(runner.decision_filter_center(geometry, "KEEP"), (403, 154))
            self.assertEqual(runner.decision_filter_center(geometry, "ALL"), (292, 154))
            with self.assertRaisesRegex(ValueError, "unknown decision toolbar action"):
                runner.decision_button_center(geometry, "MAYBE", Path("unused.png"))
            with self.assertRaisesRegex(ValueError, "unknown decision filter"):
                runner.decision_filter_center(geometry, "MAYBE")
            runner.check_catalog(database, "generated-id", {"photo-01": "UNPROCESSED"})
            runner.check_counts(
                database, "generated-id", keep=0, reject=0, unrated=13, total=13
            )

    def test_toolbar_anchor_follows_keep_chip_in_both_sprint_layouts(self):
        geometry = {"x": 1, "y": 20, "width": 1440, "height": 900}
        self.assertTrue(runner.is_keep_chip_color((35, 149, 132)))
        self.assertFalse(runner.is_keep_chip_color((35, 90, 132)))
        for keep_x, expected_keep in ((770, (787, 75)), (912, (929, 75))):
            with self.subTest(keep_x=keep_x):
                pixels = [
                    (x, y)
                    for x in range(keep_x, keep_x + 32)
                    for y in range(47, 64)
                ]
                local_center = runner.keep_chip_center_from_pixels(pixels)
                self.assertEqual(local_center, (keep_x + 16, 55))
                self.assertEqual(
                    (geometry["x"] + local_center[0], geometry["y"] + local_center[1]),
                    expected_keep,
                )
                self.assertEqual(
                    (geometry["x"] + local_center[0] + 76,
                     geometry["y"] + local_center[1]),
                    (expected_keep[0] + 76, expected_keep[1]),
                )

    def test_decision_targets_use_two_visible_photos_when_ids_do_not_match_names(self):
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
                        ("generated-id", f"id-{14 - index:02}", f"photo-{index:02}", "UNPROCESSED")
                        for index in range(1, 14)
                    ],
                )

            display_names = runner.first_page_display_names(database, "generated-id")
            targets = runner.decision_targets(display_names)

            self.assertNotIn("photo-01.jpg", display_names)
            self.assertEqual(targets, {"keep": "photo-13.jpg", "reject": "photo-12.jpg"})
            self.assertEqual(
                runner.expected_decisions(targets),
                {"photo-13": "KEEP", "photo-12": "REJECT_TECHNICAL"},
            )
            with sqlite3.connect(database) as connection:
                connection.execute(
                    "UPDATE photos SET decision = CASE stem_name "
                    "WHEN 'photo-13' THEN 'KEEP' "
                    "WHEN 'photo-12' THEN 'REJECT_TECHNICAL' "
                    "ELSE decision END"
                )
            runner.check_catalog(
                database, "generated-id", runner.expected_decisions(targets)
            )
            with self.assertRaisesRegex(RuntimeError, "at least two unique photos"):
                runner.decision_targets(["only-photo.jpg"])

    def test_enlarged_summary_ocr_accepts_visible_aliases_and_digit_confusions(self):
        self.assertEqual(runner.summary_count_from_ocr("Yo KEPT 1", "KEPT"), 1)
        self.assertEqual(runner.summary_count_from_ocr("ALT (PICK) 0", "ALT"), 0)
        self.assertEqual(runner.summary_count_from_ocr("REJECTED 1", "REJECTED"), 1)
        self.assertEqual(runner.summary_count_from_ocr("UNRATED 1l", "UNRATED"), 11)
        self.assertEqual(runner.summary_count_from_ocr("KEEP 1", "KEPT"), 1)
        with self.assertRaisesRegex(RuntimeError, "could not read KEPT label"):
            runner.summary_count_from_ocr("1", "KEPT")


if __name__ == "__main__":
    unittest.main()
