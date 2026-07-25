"""AnalysisContext class for encapsulating photo data and cached feature representations."""

from pathlib import Path
from typing import Dict, Any, Optional, Union
import os


class AnalysisContext:
    """Execution context passed to every analyzer during pipeline run.
    
    Provides lazy-loaded access to file metadata, Pillow Image objects,
    Numpy image arrays, and shared computation state.
    """

    def __init__(
        self,
        image_path: Union[str, Path],
        image_hash: Optional[str] = None,
        exif_data: Optional[Dict[str, Any]] = None,
    ):
        self.image_path = Path(image_path).resolve()
        self.image_hash = image_hash or self.image_path.name
        self.exif_data = exif_data or {}
        
        # Lazy properties
        self._pillow_image = None
        self._numpy_array = None
        self._file_bytes = None
        
        # Shared computation cache across analyzers (e.g., histogram, grayscale conversion)
        self.shared_features: Dict[str, Any] = {}

    @property
    def file_size(self) -> int:
        """Return image file size in bytes."""
        try:
            return os.path.getsize(self.image_path)
        except OSError:
            return 0

    @property
    def file_bytes(self) -> bytes:
        """Lazy load raw bytes of the image file."""
        if self._file_bytes is None:
            with open(self.image_path, "rb") as f:
                self._file_bytes = f.read()
        return self._file_bytes

    def get_pillow_image(self):
        """Lazy load and return Pillow Image object."""
        if self._pillow_image is None:
            from PIL import Image
            self._pillow_image = Image.open(self.image_path)
        return self._pillow_image

    def get_numpy_array(self):
        """Lazy load and return Numpy RGB array."""
        if self._numpy_array is None:
            import numpy as np
            pil_img = self.get_pillow_image()
            if pil_img.mode != "RGB":
                pil_img = pil_img.convert("RGB")
            self._numpy_array = np.array(pil_img)
        return self._numpy_array

    def close(self):
        """Clean up open file handles and arrays."""
        if self._pillow_image is not None:
            try:
                self._pillow_image.close()
            except Exception:
                pass
            self._pillow_image = None
        self._numpy_array = None
        self._file_bytes = None
