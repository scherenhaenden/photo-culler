"""Focused tests for the permission-free macOS Sprint 02 acceptance runner."""

import importlib.util
import io
import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock
from unittest.mock import patch


RUNNER_PATH = Path(__file__).with_name("run_s02_macos_api.py")
SPEC = importlib.util.spec_from_file_location("run_s02_macos_api", RUNNER_PATH)
assert SPEC and SPEC.loader
runner = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(runner)


class Sprint02MacOSRunnerTests(unittest.TestCase):
    def test_catalog_page_two_uses_the_returned_gallery_cursor(self):
        first_items = [{"id": f"photo-{index:02}"} for index in range(1, 13)]
        cursor = {
            "filter": {"gallery_id": "gallery-1", "decision": None},
            "after_id": "photo-12",
        }
        with patch.object(
            runner,
            "request",
            side_effect=[
                {"items": first_items, "next_cursor": cursor},
                {"items": [{"id": "photo-13"}], "next_cursor": None},
            ],
        ) as request:
            first, second = runner.catalog_pages("http://127.0.0.1:9", "token", "gallery-1")

        self.assertEqual(len(first), 12)
        self.assertEqual([row["id"] for row in second], ["photo-13"])
        self.assertIn("cursor=", request.call_args_list[1].args[2])

    def test_catalog_page_two_requires_a_second_page_cursor(self):
        with (
            patch.object(
                runner,
                "request",
                return_value={"items": [{"id": f"photo-{index:02}"} for index in range(1, 13)], "next_cursor": None},
            ),
            self.assertRaisesRegex(RuntimeError, "expected 12 entries and a next cursor"),
        ):
            runner.catalog_pages("http://127.0.0.1:9", "token", "gallery-1")

    def test_import_requires_thirteen_clean_completed_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            with patch.object(
                runner,
                "request",
                side_effect=[
                    {"total_files": 13, "logical_photos": 13},
                    {"job_id": "job-1"},
                    {"state": "completed", "discovered": 13, "imported": 13, "issues": 0},
                ],
            ) as request:
                job_id = runner.import_source(
                    "http://127.0.0.1:9", "token", Path(temporary) / "catalog.sqlite3", "gallery-1", "source-1"
                )

        self.assertEqual(job_id, "job-1")
        self.assertEqual(request.call_args_list[0].args[2], "/api/v2/import-estimates")
        self.assertEqual(request.call_args_list[1].args[2], "/api/v2/galleries/gallery-1/imports")

    def test_import_rejects_a_partial_or_unhealthy_estimate(self):
        with (
            patch.object(runner, "request", return_value={"total_files": 12, "logical_photos": 12}),
            self.assertRaisesRegex(RuntimeError, "expected 13 JPEGs"),
        ):
            runner.import_source(
                "http://127.0.0.1:9", "token", Path("catalog.sqlite3"), "gallery-1", "source-1"
            )

    def test_desktop_ready_event_must_name_the_expected_catalog_photo(self):
        with tempfile.TemporaryDirectory() as temporary:
            status = Path(temporary) / "desktop.status.tsv"
            status.write_text("gallery-loading\t\t\ngallery-ready\tphoto-01\thttp://127.0.0.1:8765\t123\n")

            self.assertEqual(runner.wait_for_desktop_ready(status, "photo-01"), "photo-01")
            with self.assertRaisesRegex(RuntimeError, "expected 'photo-02'"):
                runner.wait_for_desktop_ready(status, "photo-02")

    def test_server_uses_an_ephemeral_loopback_port_and_reads_ready_event(self):
        ready = b'{"event":"ready","origin":"http://127.0.0.1:49231","session_token":"token"}\n'
        process = MagicMock()
        process.stdout = io.BytesIO(ready)
        process.poll.return_value = None
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with (
                patch.object(runner.subprocess, "Popen", return_value=process) as popen,
                patch.object(runner.select, "select", return_value=([process.stdout], [], [])),
            ):
                started, origin, token, log = runner.start_server(
                    root / "photo-culler-server", root / "catalog", root / "source", root, "test"
                )
                self.assertIs(started, process)
                self.assertEqual((origin, token), ("http://127.0.0.1:49231", "token"))
                self.assertEqual(popen.call_args.args[0][1:3], ["--port", "0"])
                log.close()

    def test_invalid_ready_event_closes_server_stdin_and_log(self):
        process = MagicMock()
        process.stdout = io.BytesIO(b'{"event":"ready","origin":"http://example.test","session_token":"token"}\n')
        process.poll.return_value = None
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            with (
                patch.object(runner.subprocess, "Popen", return_value=process),
                patch.object(runner.select, "select", return_value=([process.stdout], [], [])),
            ):
                with self.assertRaisesRegex(RuntimeError, "invalid ready event"):
                    runner.start_server(
                        root / "photo-culler-server", root / "catalog", root / "source", root, "invalid"
                    )

        process.stdin.close.assert_called_once()
        process.wait.assert_called_once_with(timeout=5)


if __name__ == "__main__":
    unittest.main()
