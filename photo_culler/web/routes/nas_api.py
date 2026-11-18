"""FastAPI REST API endpoints for querying and configuring the NAS Thermal Monitor."""

import logging
from typing import Optional
from fastapi import APIRouter, Body, HTTPException, Request
from pydantic import BaseModel, Field

logger = logging.getLogger(__name__)
router = APIRouter(prefix="/api/v1/nas", tags=["nas"])


class NASConfigPayload(BaseModel):
    high_temp_threshold: Optional[float] = Field(
        default=None, ge=30.0, le=120.0, description="High temperature threshold in °C"
    )
    low_temp_threshold: Optional[float] = Field(
        default=None, ge=10.0, le=100.0, description="Low temperature threshold in °C"
    )
    interval: Optional[float] = Field(default=None, ge=1.0, le=3600.0, description="Polling interval in seconds")
    monitoring_enabled: Optional[bool] = Field(
        default=None, description="Enable or disable active background thermal monitoring"
    )


@router.get("/status")
def get_nas_status(request: Request) -> dict:
    """Retrieve the current dynamic temperature and thermal status of the NAS."""
    nas_manager = getattr(request.app.state, "nas_manager", None)
    if nas_manager is None:
        raise HTTPException(status_code=501, detail="NAS Thermal Monitoring is not initialized on this instance.")
    return nas_manager.snapshot()


@router.post("/config")
def update_nas_config(request: Request, payload: NASConfigPayload = Body(...)) -> dict:
    """Dynamically update NAS thermal thresholds, interval, or state."""
    nas_manager = getattr(request.app.state, "nas_manager", None)
    if nas_manager is None:
        raise HTTPException(status_code=501, detail="NAS Thermal Monitoring is not initialized on this instance.")

    if payload.high_temp_threshold is not None and payload.low_temp_threshold is not None:
        if payload.low_temp_threshold >= payload.high_temp_threshold:
            raise HTTPException(
                status_code=422,
                detail="Low temperature threshold must be strictly lower than high temperature threshold.",
            )

    # Validate against current values to avoid inconsistent state
    current_high = (
        payload.high_temp_threshold if payload.high_temp_threshold is not None else nas_manager.high_temp
    )
    current_low = payload.low_temp_threshold if payload.low_temp_threshold is not None else nas_manager.low_temp
    if current_low >= current_high:
        raise HTTPException(
            status_code=422,
            detail="Low temperature threshold must be strictly lower than high temperature threshold.",
        )

    try:
        nas_manager.set_config(
            high_temp=payload.high_temp_threshold,
            low_temp=payload.low_temp_threshold,
            interval=payload.interval,
            enabled=payload.monitoring_enabled,
        )
    except Exception as e:
        logger.exception("Failed to update NAS config")
        raise HTTPException(status_code=500, detail=f"Failed to update NAS configuration: {e}")

    return {
        "status": "ok",
        "message": "NAS configuration updated successfully.",
        "config": nas_manager.snapshot(),
    }
