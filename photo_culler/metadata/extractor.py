"""Metadata extractor for reading EXIF headers from image files."""

from PIL import Image, ExifTags
from pathlib import Path
from datetime import datetime
from typing import Optional

from ..core.models import MetadataRecord


class MetadataExtractor:
    """Extracts EXIF metadata tags from photo files."""

    def extract(self, path: Path) -> MetadataRecord:
        record = MetadataRecord()
        try:
            with Image.open(path) as img:
                exif = img._getexif()
                if not exif:
                    return record

                # Map EXIF tag IDs to human-readable names
                exif_data = {}
                for tag_id, value in exif.items():
                    tag_name = ExifTags.TAGS.get(tag_id, tag_id)
                    exif_data[tag_name] = value

                # Extract Capture Time
                dt_str = exif_data.get("DateTimeOriginal") or exif_data.get("DateTime")
                if dt_str:
                    try:
                        record.capture_time = datetime.strptime(str(dt_str), "%Y:%m:%d %H:%M:%S")
                    except ValueError:
                        pass

                record.subsecond = str(exif_data.get("SubSecTimeOriginal", "")) or None
                record.camera_make = str(exif_data.get("Make", "")).strip() or None
                record.camera_model = str(exif_data.get("Model", "")).strip() or None
                record.lens = str(exif_data.get("LensModel", "")).strip() or None

                # ISO
                iso_val = exif_data.get("ISOSpeedRatings")
                if isinstance(iso_val, (int, float)):
                    record.iso = int(iso_val)
                elif isinstance(iso_val, (list, tuple)) and len(iso_val) > 0:
                    record.iso = int(iso_val[0])

                # Aperture (FNumber)
                fnum = exif_data.get("FNumber")
                if fnum:
                    try:
                        record.aperture = float(fnum)
                    except (ValueError, TypeError):
                        pass

                # Shutter Speed (ExposureTime)
                exp = exif_data.get("ExposureTime")
                if exp:
                    if isinstance(exp, tuple) and len(exp) == 2 and exp[1] != 0:
                        record.shutter_speed = f"{exp[0]}/{exp[1]}"
                    else:
                        record.shutter_speed = str(exp)

                # Focal Length
                fl = exif_data.get("FocalLength")
                if fl:
                    try:
                        record.focal_length = float(fl)
                    except (ValueError, TypeError):
                        pass

                # Orientation
                record.orientation = int(exif_data.get("Orientation", 1))

        except Exception:
            pass

        # Fallback capture time to file modified timestamp if missing
        if record.capture_time is None:
            try:
                mtime = path.stat().st_mtime
                record.capture_time = datetime.fromtimestamp(mtime)
            except OSError:
                pass

        return record
