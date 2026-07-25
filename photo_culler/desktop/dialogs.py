"""Native Desktop Dialog Helpers."""

from typing import Optional


def select_folder_dialog(window=None) -> Optional[str]:
    """Open native OS directory selection dialog via pywebview."""
    if window:
        try:
            import webview

            result = window.create_file_dialog(webview.FOLDER_DIALOG)
            if result and len(result) > 0:
                return result[0]
        except Exception:
            pass
    return None
