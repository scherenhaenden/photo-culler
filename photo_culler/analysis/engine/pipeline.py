"""Pipeline orchestration engine for executing registered analyzers."""

from typing import List, Dict, Any, Optional
from pathlib import Path

from .context import AnalysisContext
from .result import AnalysisResult
from .analyzer import Analyzer
from .registry import AnalyzerRegistry, default_registry
from .cache import MetricCache


class AnalysisPipeline:
    """Pipeline runner executing registered analyzers against image contexts."""

    def __init__(
        self,
        registry: Optional[AnalyzerRegistry] = None,
        cache: Optional[MetricCache] = None,
        use_cache: bool = True
    ):
        self.registry = registry or default_registry
        self.cache = cache
        self.use_cache = use_cache

    def run_image(
        self,
        image_path: Path,
        image_hash: Optional[str] = None,
        analyzers: Optional[List[Analyzer]] = None,
        exif_data: Optional[Dict[str, Any]] = None,
    ) -> Dict[str, AnalysisResult]:
        """Run all requested (or default registered) analyzers on a single image.
        
        Returns:
            Dict mapping analyzer names to AnalysisResult.
        """
        context = AnalysisContext(image_path=image_path, image_hash=image_hash, exif_data=exif_data)
        
        if analyzers is None:
            analyzers = self.registry.instantiate_all(enabled_only=True)
            
        results: Dict[str, AnalysisResult] = {}
        
        try:
            for analyzer in analyzers:
                # 1. Check cache if enabled
                cached_result = None
                if self.use_cache and self.cache:
                    cached_result = self.cache.get(
                        image_hash=context.image_hash,
                        analyzer_name=analyzer.name,
                        analyzer_version=analyzer.version
                    )
                
                if cached_result:
                    results[analyzer.name] = cached_result
                    continue

                # 2. Execute analyzer
                result = analyzer.run(context)
                results[analyzer.name] = result

                # 3. Cache metric if valid and caching enabled
                if self.use_cache and self.cache and not result.error:
                    self.cache.put(image_hash=context.image_hash, result=result)
                    
        finally:
            context.close()
            
        return results
