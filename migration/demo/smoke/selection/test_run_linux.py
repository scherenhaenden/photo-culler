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
    def test_visible_text_center_finds_one_normalized_label(self):
        rows = [
            {"text": "Inspeccionar foco", "x": 100, "y": 40, "width": 120, "height": 20},
            {"text": "Photo 13", "x": 30, "y": 80, "width": 70, "height": 18},
        ]
        self.assertEqual(runner.visible_text_center(rows, "inspeccionar FOCO"), (160, 50))
        with self.assertRaisesRegex(RuntimeError, "expected one visible"):
            runner.visible_text_center(rows, "volver a galería")

    def test_inspector_button_uses_a_focused_crop_for_low_contrast_text(self):
        geometry = {"x": 11, "y": 23, "width": 1440, "height": 900}
        with mock.patch.object(runner, "enlarged_crop_ocr", return_value="Inspeccionar foco -") as ocr:
            self.assertEqual(
                runner.inspector_button_center(Path("reject-first.png"), geometry),
                (803, 95),
            )
        ocr.assert_called_once_with(Path("reject-first.png"), (720, 36, 216, 72))
        with mock.patch.object(runner, "enlarged_crop_ocr", return_value="texto ilegible"):
            with self.assertRaisesRegex(RuntimeError, "action was not visible"):
                runner.inspector_button_center(Path("reject-first.png"), geometry)

    def test_inspector_overlay_asserts_identity_decision_and_focus_controls(self):
        rows = [
            {"text": "INSPECCIÓN DE FOCO · photo-13.jpg", "x": 10, "y": 10},
            {"text": "REJECTED", "x": 900, "y": 10},
            {"text": "Zoom 100%", "x": 700, "y": 850},
            {"text": "Recorte −", "x": 600, "y": 850},
            {"text": "Restablecer", "x": 800, "y": 850},
        ]
        self.assertEqual(
            runner.assert_inspector_overlay(rows, "photo-13.jpg", "REJECTED"),
            {"photo_name": "photo-13.jpg", "decision": "REJECTED", "zoom": "100%"},
        )
        with self.assertRaisesRegex(RuntimeError, "did not identify"):
            runner.assert_inspector_overlay(rows, "photo-12.jpg", "REJECTED")
        with self.assertRaisesRegex(RuntimeError, "did not show the 'UNRATED'"):
            runner.assert_inspector_overlay(rows, "photo-13.jpg", "UNRATED")

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
            self.assertEqual(
                runner.second_page_last_photo(database, "generated-id"),
                ("photo-13", "photo-13"),
            )
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
                runner.photo_id_for_stem(database, "generated-id", "photo-13"),
                "id-01",
            )
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

    def test_keep_filter_uses_single_page_count_and_expected_inspector_photo_id(self):
        photo_id = "fd8cd4a81faff7ef163e68206163706f"
        rows = [{"text": photo_id[:30], "x": 1076, "y": 66}]
        with mock.patch.object(runner, "enlarged_crop_ocr", return_value="1 ON THIS PAGE :"):
            evidence = runner.assert_keep_filter(Path("keep-filter.png"), rows, photo_id)
        self.assertEqual(
            evidence,
            {
                "visible_photo_count": 1,
                "photo_id_prefix": photo_id[:24],
                "matching_inspector_id_rows": 1,
            },
        )

        with mock.patch.object(runner, "enlarged_crop_ocr", return_value="2 ON THIS PAGE"):
            with self.assertRaisesRegex(RuntimeError, "expected exactly one"):
                runner.assert_keep_filter(Path("keep-filter.png"), rows, photo_id)
        with mock.patch.object(runner, "enlarged_crop_ocr", return_value="1 ON THIS PAGE"):
            with self.assertRaisesRegex(RuntimeError, "expected photo ID prefix"):
                runner.assert_keep_filter(
                    Path("keep-filter.png"), [{"text": "not-the-photo", "x": 1076, "y": 66}], photo_id
                )

    def test_inspector_photo_id_ocr_tolerates_two_errors_but_rejects_other_ids(self):
        photo_id = "8c2999e5a9393c9850cbeee4f435bec9"
        # Captured from the S05 Linux artifact: OCR split the ID across rows and
        # introduced a substitution/deletion, while the screenshot shows this ID.
        self.assertTrue(
            runner.photo_id_prefix_matches_ocr(
                photo_id, "8c2999e5a9393c9850cbeeedf435!"
            )
        )
        self.assertTrue(
            runner.photo_id_prefix_matches_ocr(
                photo_id, "82999e5a9303c9850cbeee4f435bec9"
            )
        )
        self.assertTrue(
            runner.photo_id_prefix_matches_ocr(
                "e392b2358eb6768e77239a41742a7b35",
                "€392b2358eb6768e77239a41742¢",
            )
        )
        self.assertTrue(
            runner.photo_id_prefix_matches_ocr(
                "f98f20a1ef6514bae5d523dfc97a103a",
                "£98f20alef6514bae5d523dfc97al(",
            )
        )
        self.assertTrue(runner.photo_id_prefix_matches_ocr(photo_id, photo_id))
        self.assertFalse(
            runner.photo_id_prefix_matches_ocr(
                photo_id, "ffffffffffffffffffffffffffffffff"
            )
        )
        self.assertFalse(
            runner.photo_id_prefix_matches_ocr(
                photo_id, "8c2999e5a9393c9850aaaa000000000"
            )
        )

    def test_page_two_assertion_uses_catalog_photo_id_and_visible_counter(self):
        photo_id = "ef96369c76b70738e1c78d02b833f8bb"
        self.assertEqual(runner.photo_counter_from_ocr("« | Foto13de13 | > |"), (13, 13))
        rows = [{"text": photo_id, "x": 1076, "y": 122}]
        with mock.patch.object(runner, "enlarged_crop_ocr", return_value="Foto13de13"):
            evidence = runner.assert_last_photo_visible(
                Path("page-two.png"), rows, photo_id, total_photos=13
            )
        self.assertEqual(
            evidence,
            {
                "visible_photo_index": 13,
                "visible_photo_total": 13,
                "photo_id_prefix": photo_id[:24],
                "matching_inspector_id_rows": 1,
            },
        )
        with mock.patch.object(runner, "enlarged_crop_ocr", return_value="Foto12de13"):
            with self.assertRaisesRegex(RuntimeError, "expected 13 of 13"):
                runner.assert_last_photo_visible(
                    Path("page-two.png"), rows, photo_id, total_photos=13
                )
        with mock.patch.object(runner, "enlarged_crop_ocr", return_value="Foto13de13"):
            with self.assertRaisesRegex(RuntimeError, "expected photo ID prefix"):
                runner.assert_last_photo_visible(
                    Path("page-two.png"), [], photo_id, total_photos=13
                )


if __name__ == "__main__":
    unittest.main()
