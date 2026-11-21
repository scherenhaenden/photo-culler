"""Common import enums/types."""

from enum import Enum


class CancelResult(str, Enum):
    """Outcome of a cooperative cancellation request."""

    NOT_FOUND = "not_found"
    NOT_CANCELLABLE = "not_cancellable"
    CANCEL_REQUESTED = "cancel_requested"


class PauseResult(str, Enum):
    """Outcome of a cooperative pause request."""

    NOT_FOUND = "not_found"
    NOT_PAUSABLE = "not_pausable"
    PAUSE_REQUESTED = "pause_requested"


class ResumeResult(str, Enum):
    """Outcome of a persisted import resume request."""

    NOT_FOUND = "not_found"
    NOT_RESUMABLE = "not_resumable"
    SOURCE_UNAVAILABLE = "source_unavailable"
    RESUMED = "resumed"
