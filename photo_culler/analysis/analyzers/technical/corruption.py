"""Corruption Analyzer for detecting truncated, corrupt, or unreadable photo files."""

from PIL import Image
import os
from ...engine.analyzer import Analyzer
from ...engine.context import AnalysisContext
from ...engine.result import AnalysisResult


class CorruptionAnalyzer(Analyzer):
    """Verifies file decodability, byte structure, and corruption status."""

    name = "corruption"
    version = "1.0"
    category = "technical"

    def analyze(self, context: AnalysisContext) -> AnalysisResult:
        metrics = {
            "is_decodable": False,
            "corruption_status": "corrupted",
            "file_size_bytes": context.file_size,
            "is_empty": context.file_size == 0,
            "unexpected_eof": False,
            "error_detail": None,
        }

        if context.file_size == 0:
            metrics["error_detail"] = "Empty file (0 bytes)"
            return AnalysisResult(
                analyzer=self.name,
                version=self.version,
                metrics=metrics,
                confidence=1.0,
            )

        try:
            # Check opening and verifying file stream
            img = Image.open(context.image_path)
            img.verify()
            
            # Re-open for test load since verify() mutates internal file pointer
            img_load = Image.open(context.image_path)
            img_load.load()
            
            metrics["is_decodable"] = True
            metrics["corruption_status"] = "healthy"
            metrics["width"] = img_load.width
            metrics["height"] = img_load.height
            img_load.close()
            img.close()
        except OSError as os_err:
            err_msg = str(os_err).lower()
            metrics["error_detail"] = str(os_err)
            if "truncated" in err_msg or "eof" in err_msg or "broken" in err_msg:
                metrics["unexpected_eof"] = True
                metrics["corruption_status"] = "probably_corrupted"
            else:
                metrics["corruption_status"] = "corrupted"
        except Exception as exc:
            metrics["error_detail"] = str(exc)
            metrics["corruption_status"] = "corrupted"

        return AnalysisResult(
            analyzer=self.name,
            version=self.version,
            metrics=metrics,
            confidence=0.98 if metrics["is_decodable"] else 0.90,
        )
