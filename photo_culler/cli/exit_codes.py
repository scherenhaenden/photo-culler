"""Standardized CLI exit codes for photo-culler."""

from enum import IntEnum


class ExitCode(IntEnum):
    SUCCESS = 0
    GENERAL_ERROR = 1
    INVALID_ARGS = 2
    CATALOG_NOT_FOUND = 3
    VOLUME_OFFLINE = 4
    FILE_MISSING = 5
    MISSING_TOOL = 6
    PARTIAL_FAILURE = 7
    INTEGRITY_ERROR = 8
    CANCELLED = 9
    PERMISSION_DENIED = 10
