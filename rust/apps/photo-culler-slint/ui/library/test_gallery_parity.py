"""Structural parity checks; behavior is exercised by slint_view.rs unit tests."""

from pathlib import Path
import unittest

UI = Path(__file__).parent.parent
GALLERY = (UI / "library" / "gallery.slint").read_text(encoding="utf-8")
WINDOW = (UI / "demo_window.slint").read_text(encoding="utf-8")
SIDEBARS = (UI / "library" / "photo_culler_pro_sidebars.slint").read_text(encoding="utf-8")
VIEW = (UI / "../src/demo/slint_view.rs").resolve().read_text(encoding="utf-8")
CAPTURE = (UI / "library" / "capture_smoke.mjs").read_text(encoding="utf-8")
HEADER = (UI / "library" / "photo_culler_pro_header.slint").read_text(encoding="utf-8")
ICONS = (UI / "library" / "photo_culler_pro_icon.slint").read_text(encoding="utf-8")


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

    def test_selection_toolbar_actions_are_keyboard_accessible(self):
        actions_start = GALLERY.index("component DecisionAction")
        actions_end = GALLERY.index("component GalleryCard", actions_start)
        actions = GALLERY[actions_start:actions_end]

        for contract in (
            "component DecisionAction inherits FocusScope",
            "component SelectAllAction inherits FocusScope",
            "focus-on-tab-navigation: true;",
            "accessible-role: button;",
            "accessible-label:",
            "event.text == Key.Return || event.text == \" \"",
            "root.focus(); root.activated(root.decision);",
            "root.focus(); root.activated();",
            "border-width: root.has-focus ? 2px : 0px;",
            "border-color: root.has-focus ? DesignTokens.focus : #ffffff19;",
        ):
            with self.subTest(contract=contract):
                self.assertIn(contract, actions)

        for contract in (
            'accessible-name: "Keep selected photos";',
            'accessible-name: "Mark selected photos as alternate";',
            'accessible-name: "Reject selected photos";',
            'accessible-name: "Clear decisions for selected photos";',
            'accessible-label: root.all-selected ? "Deselect all visible photos" : "Select all visible photos";',
            "activated => { root.select-all-requested(); }",
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

    def test_decision_actions_are_wired_to_the_persistent_review_callback(self):
        self.assertNotIn('text: "READ ONLY";', GALLERY)
        self.assertIn('label: "KEEP"; decision: "keep";', GALLERY)
        self.assertIn('label: "ALT"; decision: "alt";', GALLERY)
        self.assertIn('label: "REJECT"; decision: "reject";', GALLERY)
        self.assertIn('label: "CLEAR"; decision: "undecided";', GALLERY)
        self.assertIn('root.decision-requested(decision)', GALLERY)

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
        for state in ("PanelState.loading", "PanelState.empty", "PanelState.error"):
            self.assertIn(state, GALLERY)
        self.assertIn("retry => { root.retry(); }", WINDOW)
        self.assertIn("select-photo(id) => { root.select-photo(id); }", WINDOW)
        self.assertIn('import { PhotoCullerProHeader } from "./library/photo_culler_pro_header.slint";', WINDOW)
        self.assertIn('import { CameraFilterOption, PhotoCullerProPrimarySidebar, PhotoCullerProFilterSidebar } from "./library/photo_culler_pro_sidebars.slint";', WINDOW)
        self.assertIn("PhotoCullerProPrimarySidebar {", WINDOW)
        self.assertIn("PhotoCullerProFilterSidebar {", WINDOW)
        self.assertIn("width: 220px;", SIDEBARS)
        self.assertIn("width: 230px;", SIDEBARS)
        self.assertIn("vertical-stretch: 0;", SIDEBARS)
        self.assertIn("viewport-width: root.width / 1px;", WINDOW)
        self.assertIn("search-query: root.search-query;", WINDOW)

    def test_shell_icons_use_native_vector_paths_for_reference_controls(self):
        expected = (
            "database", "layout-grid", "layers", "zap", "calendar", "activity",
            "hard-drive", "circle-check", "tag", "circle-x", "circle-help",
            "search", "cpu", "settings", "upload", "download", "filter",
            "rotate-ccw", "check", "user-round",
        )
        for icon_name in expected:
            with self.subTest(icon_name=icon_name):
                self.assertIn(f'root.name == "{icon_name}"', ICONS)

        self.assertIn("export component PhotoCullerProIcon inherits Rectangle", ICONS)
        self.assertIn("Path {", ICONS)
        self.assertIn("commands: root.path-data();", ICONS)
        self.assertIn("stroke: root.tint;", ICONS)
        self.assertIn("stroke-line-cap: round;", ICONS)
        self.assertIn("stroke-line-join: round;", ICONS)
        self.assertIn('name: "search";', HEADER)
        self.assertIn('icon-name: "upload";', HEADER)
        self.assertIn('icon-name: "download";', HEADER)
        self.assertIn('icon-name: "cpu";', HEADER)
        self.assertIn('icon-name: "zap";', HEADER)
        self.assertIn('icon-name: "circle-help";', HEADER)
        self.assertIn('icon-name: "settings";', HEADER)
        for icon_name in expected[:11]:
            with self.subTest(sidebar_icon=icon_name):
                self.assertIn(f'"{icon_name}"', SIDEBARS)

    def test_icon_letter_and_unicode_proxies_are_absent_from_shell(self):
        for obsolete in (
            'symbol: "P";',
            'symbol: "Z";',
            'symbol: "?";',
            'symbol: "S";',
            'text: "DB";',
            'text: "⌕";',
            'text: "↑";',
            'text: "↓";',
            'text: "↺ RESET";',
            'icon: "L";',
            'icon: "4";',
            'icon: "B";',
            'icon: "T";',
            'icon: "C";',
            'icon: "S";',
            'icon: "✓";',
            'icon: "◆";',
            'icon: "×";',
            'icon: "?";',
            'text: "F";',
            'text: root.checked ? "✓" : "";',
            'text: "OP";',
            'icon: "F";',
            'icon: "L";',
        ):
            with self.subTest(obsolete=obsolete):
                self.assertNotIn(obsolete, HEADER + SIDEBARS)
        self.assertIn('name: "check";', SIDEBARS)
        self.assertIn('name: "user-round";', SIDEBARS)
        self.assertIn("accessible-role: button;", HEADER)
        self.assertIn("accessible-label:", HEADER)


if __name__ == "__main__":
    unittest.main()
