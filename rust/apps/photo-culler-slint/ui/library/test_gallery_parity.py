"""Source contract checks for the S05 native/web library composition."""
from pathlib import Path
import unittest

UI = Path(__file__).parent.parent
TOKENS = (UI / "components" / "components.slint").read_text(encoding="utf-8")
GALLERY = (UI / "library" / "gallery.slint").read_text(encoding="utf-8")
WINDOW = (UI / "demo_window.slint").read_text(encoding="utf-8")


class GalleryParityTests(unittest.TestCase):
    def test_dark_reference_palette_and_surface_specific_decisions(self):
        for role, color in (
            ("canvas", "#0f1012"),
            ("surface", "#111111"),
            ("card-hover", "#1c1d21"),
            ("sidebar", "#050505"),
            ("accent", "#ff6b35"),
            ("foreground", "#e3e2e4"),
            ("best", "#25a18e"),
            ("keep", "#58a6ff"),
            ("rejected", "#e63946"),
            ("web-rejected", "#ff6b35"),
        ):
            with self.subTest(role=role):
                self.assertIn(f"out property <color> {role}: dark-mode ? {color}", TOKENS)
        self.assertIn("in-out property <bool> dark-mode: true;", TOKENS)
        self.assertIn("web-layout ? #ffffff19 : #191919", TOKENS)
        self.assertIn("root.web-layout ? DesignTokens.web-rejected : DesignTokens.rejected", GALLERY)

    def test_native_preview_filmstrip_inspector_and_web_responsive_grid(self):
        self.assertIn("if root.view-state == 1 && !root.web-layout: HorizontalLayout", GALLERY)
        self.assertIn("if root.view-state == 1 && root.web-layout: HorizontalLayout", GALLERY)
        self.assertIn('text: "TIRA DE FOTOS";', GALLERY)
        self.assertIn("horizontal-scrollbar-policy: ScrollBarPolicy.as-needed;", GALLERY)
        self.assertIn("content-width: 236px * root.item-count;", GALLERY)
        self.assertIn("property <length> inspector-width: 280px;", GALLERY)
        self.assertIn("width: 320px; height: parent.height;", GALLERY)
        self.assertIn("root.inspector-width = self.requested-width < 260px ? 260px", GALLERY)
        self.assertIn("min-width: web-layout ? 200px : 0px;", GALLERY)
        self.assertIn("height: 180px;", GALLERY)
        self.assertIn("image-fit: cover;", GALLERY)
        self.assertIn("image-fit: contain;", GALLERY)
        self.assertIn("focus-on-tab-navigation: true;", GALLERY)
        self.assertIn("focus_scope.has-focus ? 2px : 0px", GALLERY)
        self.assertIn("root.width < 440px", GALLERY)
        self.assertIn("root.width >= 680px", GALLERY)

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
        ):
            with self.subTest(public=public):
                self.assertIn(public, WINDOW)
        for state in ("PanelState.loading", "PanelState.empty", "PanelState.error"):
            self.assertIn(state, WINDOW)
        self.assertIn("retry => { root.retry(); }", WINDOW)
        self.assertIn("select-photo(id) => { root.select-photo(id); }", WINDOW)
        self.assertIn("property <length> sidebar-width: 220px;", WINDOW)
        self.assertIn("root.sidebar-width = self.requested-width < 200px ? 200px", WINDOW)


if __name__ == "__main__":
    unittest.main()
