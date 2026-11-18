"""Metadata Extractor reading EXIF data, camera settings, and orientation tags."""

from datetime import datetime
from pathlib import Path
from typing import Union

from PIL import ExifTags, Image

from photo_culler.core.models import MetadataRecord


class MetadataExtractor:
    """Extracts technical EXIF metadata from photo files using Pillow and ExifTags."""

    def extract(self, file_path: Union[str, Path]) -> MetadataRecord:
        path = Path(file_path)
        capture_time = datetime.fromtimestamp(path.stat().st_mtime)
        camera_make = None
        camera_model = None
        lens = None
        iso = None
        aperture = None
        shutter_speed = None
        focal_length = None
        orientation = 1

        try:
            with Image.open(path) as img:
                exif_raw = img._getexif()
                if exif_raw:
                    exif_data = {ExifTags.TAGS.get(tag, tag): val for tag, val in exif_raw.items()}

                    # Orientation
                    try:
                        orientation = int(exif_data.get("Orientation", 1))
                    except (TypeError, ValueError):
                        orientation = 1

                    # Camera make & model
                    camera_make = exif_data.get("Make")
                    camera_model = exif_data.get("Model")
                    lens = exif_data.get("LensModel") or exif_data.get("Lens")

                    # ISO
                    iso_raw = exif_data.get("ISOSpeedRatings") or exif_data.get("PhotographicSensitivity")
                    if isinstance(iso_raw, (tuple, list)) and len(iso_raw) > 0:
                        iso_raw = iso_raw[0]
                    if isinstance(iso_raw, int):
                        iso = iso_raw
                    elif isinstance(iso_raw, str) and iso_raw.isdigit():
                        iso = int(iso_raw)

                    # Aperture (FNumber)
                    fnum = exif_data.get("FNumber")
                    if fnum is not None:
                        try:
                            aperture = float(fnum)
                        except (TypeError, ValueError):
                            pass

                    # Shutter Speed (ExposureTime)
                    exp_time = exif_data.get("ExposureTime")
                    if exp_time is not None:
                        try:
                            shutter_speed = str(exp_time)
                        except (TypeError, ValueError):
                            pass

                    # Focal Length
                    focal = exif_data.get("FocalLength")
                    if focal is not None:
                        try:
                            focal_length = float(focal)
                        except (TypeError, ValueError):
                            pass

                    # Date Time Original
                    dt_str = exif_data.get("DateTimeOriginal") or exif_data.get("DateTime")
                    if dt_str:
                        try:
                            capture_time = datetime.strptime(str(dt_str), "%Y:%m:%d %H:%M:%S")
                        except ValueError:
                            pass
        except Exception:
            pass

        return MetadataRecord(
            capture_time=capture_time,
            camera_make=str(camera_make) if camera_make else None,
            camera_model=str(camera_model) if camera_model else None,
            lens=str(lens) if lens else None,
            iso=iso,
            aperture=aperture,
            shutter_speed=shutter_speed,
            focal_length=focal_length,
            orientation=orientation,
        )
