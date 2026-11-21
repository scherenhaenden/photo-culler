"""Unification router for galleries, imports, photos and system REST endpoints."""

from fastapi import APIRouter
from photo_culler.web.routes.api.galleries import router as galleries_router
from photo_culler.web.routes.api.photos import router as photos_router
from photo_culler.web.routes.api.photos import get_system_usage

router = APIRouter(prefix="/api")

# Include the split-out routers
router.include_router(galleries_router)
router.include_router(photos_router)
