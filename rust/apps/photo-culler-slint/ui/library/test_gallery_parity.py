"""Source contracts for the shared Photo Culler Pro library grid."""
from pathlib import Path
import unittest

UI = Path(__file__).parent.parent
TOKENS = (UI / "components" / "components.slint").read_text(encoding="utf-8")
GALLERY = (UI / "library" / "gallery.slint").read_text(encoding="utf-8")
WINDOW = (UI / "demo_window.slint").read_text(encoding="utf-8")
SIDEBARS = (UI / "library" / "photo_culler_pro_sidebars.slint").read_text(encoding="utf-8")
HEADER = (UI / "library" / "photo_culler_pro_header.slint").read_text(encoding="utf-8")


class GalleryParityTests(unittest.TestCase):
    def test_pro_palette_and_public_gallery_contract(self):
        for role, color in (
            ("canvas", "#0f1012"),
            ("surface", "#111111"),
            ("sidebar", "#050505"),
            ("accent", "#ff6b35"),
            ("foreground", "#e3e2e4"),
            ("best", "#25a18e"),
        ):
            with self.subTest(role=role):
                self.assertIn(f"out property <color> {role}: dark-mode ? {color}", TOKENS)

        for contract in (
            "export struct GalleryPhoto {",
            "rating: int,",
            "iso: int,",
            "is-raw: bool,",
            "export component LibraryGallery inherits Rectangle {",
            "in property <bool> web-layout: false;",
            "in property <int> viewport-width: 0;",
            'in property <string> search-query: "";',
            "in property <int> view-state:",
            "in property <[GalleryPhoto]> photos: [];",
            'in property <string> fallback-id: "demo-001";',
            'in property <string> fallback-name: "demo-001.jpg";',
            "in property <image> fallback-thumbnail;",
            'in property <string> fallback-decision: "UNRATED";',
            'in property <string> selected-id: "";',
            "in property <string> error-message:",
            "callback retry();",
            "callback select-photo(string);",
        ):
            with self.subTest(contract=contract):
                self.assertIn(contract, GALLERY)

    def test_native_and_wasm_share_the_pro_responsive_grid(self):
        self.assertIn("height: 46px;", GALLERY)
        self.assertIn("spacing: 12px;", GALLERY)
        self.assertIn("height: root.card-width * 3 / 4;", GALLERY)
        self.assertIn("border-radius: 16px;", GALLERY)
        self.assertIn("image-fit: cover;", GALLERY)
        self.assertIn("root.effective-viewport-width >= 640", GALLERY)
        self.assertIn("root.effective-viewport-width >= 768", GALLERY)
        self.assertIn("root.effective-viewport-width >= 1024", GALLERY)
        self.assertIn("root.effective-viewport-width >= 1280", GALLERY)
        self.assertIn("root.effective-viewport-width >= 1024 ? 6", GALLERY)
        self.assertIn("root.grid-columns", GALLERY)
        self.assertIn("root.viewport-width > 0 ? root.viewport-width : round(root.width / 1px)", GALLERY)
        self.assertIn("border-color: root.selected ? #ff6b35", GALLERY)
        self.assertIn('text: "RAW";', GALLERY)
        self.assertIn("root.photo.score", GALLERY)
        self.assertIn("root.photo.iso", GALLERY)
        self.assertIn("root.photo.rating", GALLERY)
        self.assertIn("visible: hover-area.has-hover;", GALLERY)
        self.assertIn('text: root.item-count + " CATALOGUED";', GALLERY)
        self.assertIn('label: "KEEP"; fill-color: #25a18e;', GALLERY)
        self.assertIn('label: "ALT"; fill-color: #f7c59f;', GALLERY)
        self.assertIn('label: "REJECT"; fill-color: #ff6b35;', GALLERY)
        self.assertIn('text: root.all-selected ? "DESELECT ALL" : "SELECT ALL";', GALLERY)
        self.assertIn('label: "SMALL";', GALLERY)
        self.assertIn('label: "MED";', GALLERY)
        self.assertIn('label: "LARGE";', GALLERY)
        self.assertIn('text: "◉  INSPECT";', GALLERY)
        self.assertIn("visible: !root.filter-locally || root.matches-search;", GALLERY)
        self.assertIn("filter-locally: root.photos.length == 1;", GALLERY)
        self.assertIn("root.photos.length == 0 && root.fallback-matches-search", GALLERY)
        self.assertNotIn("if root.view-state == 1 && !root.web-layout", GALLERY)
        self.assertNotIn("if root.view-state == 1 && root.web-layout", GALLERY)

    def test_search_is_case_insensitive_and_targets_filenames(self):
        self.assertIn("in property <string> search-query: \"\";", GALLERY)
        self.assertIn("root.fallback-name.to-lowercase()", GALLERY)
        self.assertIn("root.search-query.to-lowercase()", GALLERY)
        self.assertIn("filename-lower.replace-all(root.query-lower, \"\")", GALLERY)
        self.assertIn("root.photos[0].filename.to-lowercase().replace-all(root.query-lower, \"\")", GALLERY)
        self.assertIn("filter-locally: root.photos.length == 1;", GALLERY)

    def test_old_preview_filmstrip_and_inspector_composition_is_absent(self):
        for obsolete in (
            "PhotoInspector",
            "FilmstripCard",
            'text: "TIRA DE FOTOS";',
            "inspector-width",
            "image-fit: contain;",
            "content-width: 236px * root.item-count;",
        ):
            with self.subTest(obsolete=obsolete):
                self.assertNotIn(obsolete, GALLERY)

    def test_shell_keeps_runtime_contract_and_recoverable_states(self):
        for public in (
            "in property <int> view-state:",
            "in property <string> photo-name:",
            "in property <string> error-message:",
            "in property <image> photo-thumbnail;",
            "in property <int> photo-index:",
            "in property <int> photo-count:",
            "in property <string> navigation-label:",
            "callback retry();",
            "callback previous-photo();",
            "callback next-photo();",
            "in property <[GalleryPhoto]> gallery-photos:",
            "in property <int> kept-count: 0;",
            "in property <int> alternate-count: 0;",
            "in property <int> rejected-count: 0;",
            "in property <int> unrated-count: 0;",
            "callback select-photo(string);",
            "callback select-archive(string);",
            'in property <string> fallback-photo-id: "demo-001";',
        ):
            with self.subTest(public=public):
                self.assertIn(public, WINDOW)
        for state in ("PanelState.loading", "PanelState.empty", "PanelState.error"):
            self.assertIn(state, GALLERY)
        self.assertIn("retry => { root.retry(); }", WINDOW)
        self.assertIn("select-photo(id) => { root.select-photo(id); }", WINDOW)
        self.assertIn('import { PhotoCullerProHeader } from "./library/photo_culler_pro_header.slint";', WINDOW)
        self.assertIn('import { PhotoCullerProPrimarySidebar, PhotoCullerProFilterSidebar } from "./library/photo_culler_pro_sidebars.slint";', WINDOW)
        self.assertIn("PhotoCullerProPrimarySidebar {", WINDOW)
        self.assertIn("PhotoCullerProFilterSidebar {", WINDOW)
        self.assertIn("width: 220px;", SIDEBARS)
        self.assertIn("width: 230px;", SIDEBARS)
        self.assertIn("vertical-stretch: 0;", SIDEBARS)
        self.assertIn("viewport-width: root.width / 1px;", WINDOW)
        self.assertIn("search-query: root.search-query;", WINDOW)
        for symbol in ('symbol: "P";', 'symbol: "Z";', 'symbol: "?";', 'symbol: "S";'):
            with self.subTest(symbol=symbol):
                self.assertIn(symbol, HEADER)


if __name__ == "__main__":
    unittest.main()
