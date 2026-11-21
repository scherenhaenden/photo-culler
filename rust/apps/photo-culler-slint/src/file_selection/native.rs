//! Native desktop folder picker backed by the operating system's dialog.

use super::{Intent, NativePath, NativePicker, SelectionError};

/// Uses the Linux desktop portal where available and the native macOS folder
/// chooser. `None` is preserved as cancellation by the `NativePicker` contract.
#[derive(Clone, Copy, Debug, Default)]
pub struct SystemNativePicker;

impl NativePicker for SystemNativePicker {
    fn pick(&mut self, intent: Intent) -> Result<Option<NativePath>, SelectionError> {
        let title = match intent {
            Intent::Source => "Choose photo folder",
            Intent::Destination => "Choose destination folder",
        };

        let Some(path) = rfd::FileDialog::new().set_title(title).pick_folder() else {
            return Ok(None);
        };

        NativePath::from_path(&path).map(Some)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn selected_path_preserves_unicode_and_spaces() {
        let path = NativePath::from_path(Path::new("/home/alice/Imágenes de viaje")).unwrap();
        assert_eq!(path.as_str(), "/home/alice/Imágenes de viaje");
    }

    #[cfg(unix)]
    #[test]
    fn selected_non_utf8_path_is_rejected_instead_of_lossily_converted() {
        use std::os::unix::ffi::OsStrExt;

        let raw = std::ffi::OsStr::from_bytes(b"/photos/invalid-\xff");
        assert_eq!(
            NativePath::from_path(Path::new(raw)),
            Err(SelectionError::Rejected)
        );
    }
}
