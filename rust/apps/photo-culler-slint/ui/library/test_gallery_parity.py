"""Structural parity checks; behavior is exercised by slint_view.rs unit tests."""

from pathlib import Path
import unittest

UI = Path(__file__).parent.parent
GALLERY = (UI / "library" / "gallery.slint").read_text(encoding="utf-8")
WINDOW = (UI / "demo_window.slint").read_text(encoding="utf-8")
SIDEBARS = (UI / "library" / "photo_culler_pro_sidebars.slint").read_text(encoding="utf-8")
VIEW = (UI / "../src/demo/slint_view.rs").resolve().read_text(encoding="utf-8")
CAPTURE = (UI / "library" / "capture_smoke.mjs").read_text(encoding="utf-8")


class GalleryParityTests(unittest.TestCase):
    def test_gallery_contract_carries_explicit_unknown_metadata_and_selection(self):
        for contract in (
            "export struct GalleryPhoto {",
            "rating: int,",
            "iso: int,",
            "is-raw: bool,",
            "camera: string,",
            "capture-time: string,",
            "sharpness: int,",
            "metadata-known: bool,",
            "selected: bool,",
            "callback selection-requested(photo-id: string, multi: bool);",
            "callback select-all-requested();",
        ):
            with self.subTest(contract=contract):
                self.assertIn(contract, GALLERY)

    def test_reference_shell_gallery_geometry_and_density_breakpoints(self):
        for contract in (
            "height: 46px;",
            "height: root.card-width * 3 / 4;",
            "border-radius: 16px;",
            "image-fit: cover;",
            "root.effective-viewport-width >= 640",
            "root.effective-viewport-width >= 768",
            "root.effective-viewport-width >= 1024",
            "root.effective-viewport-width >= 1280",
            "root.grid-density == 0",
            "root.grid-density == 2",
        ):
            with self.subTest(contract=contract):
                self.assertIn(contract, GALLERY)
        self.assertIn("width: 220px;", SIDEBARS)
        self.assertIn("width: 230px;", SIDEBARS)
        self.assertIn("spacing: 12px;", GALLERY)
        self.assertIn('text: "CULL";', GALLERY)
        self.assertIn("color: #ffffff04;", GALLERY)

    def test_pro_filter_ranges_match_the_reference_controls(self):
        self.assertIn("in property <bool> show-endpoints: true;", SIDEBARS)
        self.assertIn('show-endpoints: false;', SIDEBARS)
        self.assertIn('value-prefix: "≤ ";', SIDEBARS)
        self.assertIn("background: root.tint;", SIDEBARS)
        self.assertNotIn('text: "SMOKE FIXTURE · SYNTHETIC METADATA";', GALLERY)

    def test_search_filters_sort_density_and_selection_reach_the_live_adapter(self):
        for name in (
            'window.on_search_changed(move |query|',
            'window.on_filter_requested(move |name, value|',
            'window.on_selection_requested(move |photo_id, multi|',
            'window.on_select_all_requested(move ||',
            'window.on_sort_direction_changed(move |descending|',
            'window.on_density_changed(move |density|',
            '"cull-status" =>',
            '"camera-body" =>',
            '"max-iso" =>',
            '"min-sharpness" =>',
            '"min-rating" =>',
            '"raw-only" =>',
            '.chain(photo.camera.iter().map(String::as_str))',
            '.chain(photo.lens.iter().map(String::as_str))',
            '.chain(photo.tags.iter().map(String::as_str))',
            '.chain(photo.session_name.iter().map(String::as_str))',
            'state.reset_filters()',
            'state.toggle_all_visible()',
            'state.visible_indices()',
        ):
            with self.subTest(name=name):
                self.assertIn(name, VIEW)

    def test_camera_exif_filters_show_unavailable_and_do_not_take_input_live(self):
        self.assertIn('text: root.metadata-available ? "NO CAMERA VALUES" : "CAMERA FILTER · UNAVAILABLE";', SIDEBARS)
        self.assertIn('root.title + " · UNAVAILABLE"', SIDEBARS)
        self.assertIn('"RAW FORMATS ONLY · UNAVAILABLE"', SIDEBARS)
        self.assertIn('enabled: root.metadata-available;', SIDEBARS)
        self.assertIn("camera: None,", VIEW)
        self.assertIn("iso: None,", VIEW)
        self.assertIn("sharpness: None,", VIEW)
        self.assertIn("focus_accuracy: None,", VIEW)
        self.assertIn("lens: None,", VIEW)
        self.assertIn("tags: Vec::new(),", VIEW)
        self.assertIn("session_name: None,", VIEW)
        self.assertIn("rating: None,", VIEW)
        self.assertIn("is_raw: None,", VIEW)
        self.assertIn('library.metadata_available = self.smoke_fixture;', VIEW)

    def test_smoke_fixture_is_explicit_and_uses_the_rest_jpeg(self):
        self.assertIn('PHOTO_CULLER_GALLERY_SMOKE', VIEW)
        self.assertIn('photo-culler-smoke=1', VIEW)
        self.assertIn('"photo-10"', VIEW)
        self.assertIn("focus_accuracy: Option<u8>,", VIEW)
        self.assertIn(".focus_accuracy\n                .map(|score|", VIEW)
        self.assertIn("sharpness: Some(sharpness),", VIEW)
        for camera in ("NIKON Z9", "Sony A7R IV", "Canon EOS R5"):
            with self.subTest(camera=camera):
                self.assertIn(f'"{camera}"', VIEW)
        self.assertIn("ordered_camera_names(&state.items)", VIEW)
        self.assertNotIn('"Fujifilm X-T5"', VIEW)
        self.assertIn('format!("{filename} · 10-card smoke fixture")', VIEW)
        self.assertIn('EXPECTED_JPEG_SHA256 = "cc20c24844d2fc6fca729fcb66cfe661960f7b56be41a3dd462a0f1b22cfb4f2"', CAPTURE)
        self.assertIn('pro-library-1440x900.png', CAPTURE)
        self.assertIn('pro-library-1100x700.png', CAPTURE)
        self.assertIn('?photo-culler-smoke=1', CAPTURE)
        self.assertIn('canvas_width: Math.round(document.querySelector("#canvas").getBoundingClientRect().width)', CAPTURE)
        self.assertIn('canvas_backing_width: document.querySelector("#canvas").width', CAPTURE)
        self.assertIn('fullPage: false', CAPTURE)

    def test_write_actions_remain_disabled_without_an_implementation_badge(self):
        self.assertNotIn('text: "READ ONLY";', GALLERY)
        self.assertIn('DisabledAction { label: "KEEP";', GALLERY)
        self.assertIn('DisabledAction { label: "ALT";', GALLERY)
        self.assertIn('DisabledAction { label: "REJECT";', GALLERY)

    def test_1100px_header_keeps_every_reference_tab_and_control_visible(self):
        header = (UI / "library" / "photo_culler_pro_header.slint").read_text(encoding="utf-8")
        self.assertIn('if root.width >= 1100px: PhotoCullerProTab {\n                    label: "BURSTS";', header)
        self.assertIn('if root.width >= 1100px: PhotoCullerProTab {\n                    label: "DASHBOARD";', header)
        self.assertIn('width: root.width >= 1200px ? 208px : (root.width >= 900px ? 128px : 96px);', header)
        self.assertIn('width: root.width >= 1200px ? 108px : 60px;', header)
        self.assertIn('if root.width < 1200px: VerticalLayout {', header)
        self.assertIn('if root.width >= 1100px: PhotoCullerProControl {', header)

    def test_card_selection_is_rendered_from_the_adapter_selection_set(self):
        self.assertIn("selected: photo.selected;", GALLERY)
        self.assertNotIn("photo.selected || photo.id == root.selected-id", GALLERY)

    def test_root_preserves_existing_runtime_contract_and_connects_new_state(self):
        for contract in (
            "in property <int> view-state:",
            "in property <string> photo-name:",
            "in property <image> photo-thumbnail;",
            "in property <[GalleryPhoto]> gallery-photos:",
            "callback retry();",
            "callback select-photo(string);",
            "in-out property <string> cull-status:",
            "in-out property <int> grid-density:",
            "in-out property <bool> sort-descending:",
            "callback selection-requested(photo-id: string, multi: bool);",
        ):
            with self.subTest(contract=contract):
                self.assertIn(contract, WINDOW)
        self.assertIn("cull-status <=> root.cull-status;", WINDOW)
        self.assertIn("max-iso <=> root.max-iso;", WINDOW)
        self.assertIn("grid-density <=> root.grid-density;", WINDOW)
        self.assertIn("selection-requested(id, multi) => { root.selection-requested(id, multi); }", WINDOW)


if __name__ == "__main__":
    unittest.main()
