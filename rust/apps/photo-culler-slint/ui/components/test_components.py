"""Source-level contract checks until the Slint app crate is integrated."""
from pathlib import Path
import unittest


SOURCE = Path(__file__).with_name("components.slint").read_text(encoding="utf-8")


class ComponentCatalogTests(unittest.TestCase):
    def test_catalog_exports_shared_components_and_states(self):
        for declaration in (
            "export global DesignTokens",
            "export enum PanelState",
            "export component ActionButton",
            "export component SidebarItem",
            "export component StatusPanel",
            "export component Sidebar",
            "export component ComponentCatalog inherits Window",
        ):
            with self.subTest(declaration=declaration):
                self.assertIn(declaration, SOURCE)

    def test_light_and_dark_themes_have_focus_and_error_contrast(self):
        for token in ("canvas", "surface", "foreground", "muted", "border", "accent", "focus", "danger"):
            with self.subTest(token=token):
                self.assertRegex(SOURCE, rf"out property <color> {token}: dark-mode \?")
        self.assertIn("border-color: DesignTokens.focus", SOURCE)
        self.assertIn("focus_scope.has-focus ? 2px : 0px", SOURCE)

    def test_controls_support_keyboard_and_stable_callbacks(self):
        self.assertGreaterEqual(SOURCE.count("focus-on-tab-navigation: true"), 2)
        self.assertGreaterEqual(SOURCE.count("event.text == Key.Return || event.text == \" \""), 2)
        self.assertIn("callback clicked();", SOURCE)
        self.assertIn("callback activated();", SOURCE)
        self.assertIn("callback retry();", SOURCE)
        self.assertIn("callback navigate(index: int);", SOURCE)

    def test_loading_error_empty_states_are_distinct_and_retryable(self):
        for state in ("loading", "error", "empty"):
            self.assertIn(f"{state},", SOURCE)
        self.assertIn("if root.state == PanelState.loading", SOURCE)
        self.assertIn("if root.state == PanelState.error", SOURCE)
        self.assertIn("root.retry()", SOURCE)
        self.assertIn("Shared component catalog", SOURCE)


if __name__ == "__main__":
    unittest.main()
