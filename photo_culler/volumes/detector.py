"""Volume detector and marker manager."""

import json
import uuid
import os
import shutil
from pathlib import Path
from typing import Optional, Dict, Any

from ..core.models import VolumeRecord
from ..core.enums import VolumeStatus


MARKER_FILENAME = ".photo-culler-volume.json"


class VolumeDetector:
    """Manages disk volume identification and marker file persistence."""

    def get_or_create_volume_marker(self, directory: Path) -> VolumeRecord:
        """Inspect directory path, resolve volume mount point, and read/create marker file."""
        root_dir = directory.resolve()
        
        # Find mount point or fallback to root directory
        mount_point = root_dir
        while mount_point.parent != mount_point and not os.path.ismount(mount_point):
            mount_point = mount_point.parent

        marker_path = mount_point / MARKER_FILENAME
        
        if marker_path.exists():
            try:
                with open(marker_path, "r", encoding="utf-8") as f:
                    data = json.load(f)
                volume_id = data["volume_id"]
                label = data.get("label", mount_point.name or "Root")
            except Exception:
                volume_id = str(uuid.uuid4())
                label = mount_point.name or "Volume"
                self._write_marker(marker_path, volume_id, label)
        else:
            volume_id = str(uuid.uuid4())
            label = mount_point.name or "Volume"
            self._write_marker(marker_path, volume_id, label)

        total, used, free = shutil.disk_usage(mount_point)

        return VolumeRecord(
            volume_id=volume_id,
            label=label,
            mount_point=mount_point,
            total_bytes=total,
            available_bytes=free,
            is_online=True,
        )

    def _write_marker(self, marker_path: Path, volume_id: str, label: str):
        data = {
            "volume_id": volume_id,
            "label": label,
            "created_by": "photo-culler",
            "version": "1.0",
        }
        try:
            with open(marker_path, "w", encoding="utf-8") as f:
                json.dump(data, f, indent=2)
        except OSError:
            pass  # Read-only volumes fail gracefully
