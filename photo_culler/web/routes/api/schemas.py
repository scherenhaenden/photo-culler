"""Pydantic schemas/models for versioned API endpoints."""

from typing import Literal
from pydantic import BaseModel, Field, field_validator


class GalleryCreateRequest(BaseModel):
    """Create-gallery API request."""

    name: str = Field(min_length=1, max_length=255)

    @field_validator("name")
    @classmethod
    def name_must_not_be_blank(cls, value: str) -> str:
        """Reject names that contain only whitespace."""
        if not value.strip():
            raise ValueError("Gallery name cannot be empty")
        return value


class GalleryImportRequest(BaseModel):
    """Non-copying import request."""

    path: str = Field(min_length=1, max_length=2048)
    recursive: bool = True
    exclude_patterns: list[str] = Field(default_factory=list, max_length=50)


class GalleryImportEstimateRequest(BaseModel):
    """Read-only import preflight request."""

    path: str = Field(min_length=1, max_length=2048)
    recursive: bool = True
    exclude_patterns: list[str] = Field(default_factory=list, max_length=50)


class NativeDecisionRequest(BaseModel):
    """Decision mutation used by native delivery adapters."""

    decision: Literal["best", "keep", "alternate", "review", "reject", "recover"]


class NativeAnalysisStartRequest(BaseModel):
    """JSON equivalent of the analysis form used by native delivery adapters."""

    profile: str = Field(default="fast", min_length=1, max_length=128)
    scope: str = Field(default="remaining", pattern="^(remaining|all)$")
