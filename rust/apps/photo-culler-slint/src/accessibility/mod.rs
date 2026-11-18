//! Accessible startup and recovery surface for the Slint canvas application.
//!
//! Slint's browser canvas does not expose its controls to screen readers. This
//! small HTML surface provides accessible startup, failure reporting, and a
//! route to the native application. Native focus names remain the
//! responsibility of the registered Slint controls and platform adapter.

/// The keyboard route shared by gallery, inspector, and return navigation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Screen {
    Gallery,
    Inspector,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Enter,
    Escape,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusTarget {
    GalleryPhoto,
    InspectorBack,
    InspectorImage,
}

/// Minimal focus policy: Enter opens the selected photo and Escape returns to
/// the gallery, where focus is restored to the selected photo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FocusState {
    pub screen: Screen,
    pub focused: FocusTarget,
}

impl Default for FocusState {
    fn default() -> Self {
        Self {
            screen: Screen::Gallery,
            focused: FocusTarget::GalleryPhoto,
        }
    }
}

impl FocusState {
    pub fn key(self, key: Key) -> Self {
        match (self.screen, key) {
            (Screen::Gallery, Key::Enter) => Self {
                screen: Screen::Inspector,
                focused: FocusTarget::InspectorBack,
            },
            (Screen::Inspector, Key::Escape) => Self {
                screen: Screen::Gallery,
                focused: FocusTarget::GalleryPhoto,
            },
            _ => self,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StartupState {
    Starting,
    CanvasUnavailable,
    ServerUnavailable,
}

/// Static page kept independent from the Slint canvas so assistive technology
/// can announce startup/error state and expose a native-app alternative.
pub fn startup_page(state: StartupState, native_url: &str) -> String {
    let (title, message) = match state {
        StartupState::Starting => ("Starting Photo Culler", "Photo Culler is starting. Please wait."),
        StartupState::CanvasUnavailable => (
            "Photo Culler web view unavailable",
            "The browser could not initialize the visual canvas. Use the native application to review photos.",
        ),
        StartupState::ServerUnavailable => (
            "Photo Culler server unavailable",
            "The local Photo Culler service could not be reached. Check that it is running, then retry.",
        ),
    };
    let safe_url = escape_attribute(native_url);
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{title}</title></head><body><main><h1>{title}</h1><p role=\"status\" aria-live=\"polite\">{message}</p><p>The photo-review canvas does not expose its visual controls as screen-reader elements. This page covers startup and recovery only; keyboard and screen-reader access to the canvas is unavailable.</p><p><a href=\"{safe_url}\">Open the native Photo Culler application</a></p><button type=\"button\" onclick=\"location.reload()\">Retry</button></main></body></html>"
    )
}

fn escape_attribute(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\'', "&#39;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keyboard_opens_inspector_and_escape_restores_gallery_focus() {
        let gallery = FocusState::default();
        let inspector = gallery.key(Key::Enter);
        assert_eq!(inspector.screen, Screen::Inspector);
        assert_eq!(inspector.focused, FocusTarget::InspectorBack);
        let returned = inspector.key(Key::Escape);
        assert_eq!(returned, gallery);
    }

    #[test]
    fn unrelated_keys_do_not_steal_focus() {
        let state = FocusState::default();
        assert_eq!(state.key(Key::Other), state);
        assert_eq!(state.key(Key::Escape), state);
    }

    #[test]
    fn startup_error_page_announces_state_and_offers_native_alternative() {
        for state in [
            StartupState::Starting,
            StartupState::CanvasUnavailable,
            StartupState::ServerUnavailable,
        ] {
            let html = startup_page(state, "photo-culler://open");
            assert!(html.contains("<html lang=\"en\">"));
            assert!(html.contains("<main>"));
            assert!(html.contains("role=\"status\" aria-live=\"polite\""));
            assert!(html.contains("Open the native Photo Culler application"));
            assert!(html.contains("Retry</button>"));
            assert!(html.contains("screen-reader elements"));
        }
    }

    #[test]
    fn native_link_attribute_is_escaped() {
        let html = startup_page(StartupState::CanvasUnavailable, "app://open?x=\"<bad>&");
        assert!(html.contains("href=\"app://open?x=&quot;&lt;bad&gt;&amp;\""));
        assert!(!html.contains("href=\"app://open?x=\"<bad>"));
    }
}
