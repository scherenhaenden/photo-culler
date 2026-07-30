"""Core enumerations for photo-culler."""

from enum import Enum


class FileRole(str, Enum):
    RAW = "raw"
    JPEG = "jpeg"
    IMAGE = "image"
    SIDECAR = "sidecar"
    EXPORT = "export"
    UNKNOWN = "unknown"


class DecisionState(str, Enum):
    KEEP = "KEEP"
    BEST = "BEST"
    ALTERNATE = "ALTERNATE"
    REVIEW = "REVIEW"
    RECOVER = "RECOVER"
    REJECT_TECHNICAL = "REJECT_TECHNICAL"
    REJECT_REDUNDANT = "REJECT_REDUNDANT"
    PROTECTED_BY_COVERAGE = "PROTECTED_BY_COVERAGE"
    UNPROCESSED = "UNPROCESSED"


class QualityTier(str, Enum):
    EXCELLENT = "excellent"
    GOOD = "good"
    FAIR = "fair"
    POOR = "poor"
    CORRUPTED = "corrupted"


class VolumeStatus(str, Enum):
    ONLINE = "online"
    OFFLINE = "offline"
    READ_ONLY = "read_only"
