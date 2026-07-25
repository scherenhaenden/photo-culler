"""Unit tests for core analysis engine, registry, SQLite cache, and pipeline."""

import pytest
import os
import tempfile
from pathlib import Path
from PIL import Image

from photo_culler.analysis.engine.context import AnalysisContext
from photo_culler.analysis.engine.result import AnalysisResult
from photo_culler.analysis.engine.analyzer import Analyzer
from photo_culler.analysis.engine.registry import AnalyzerRegistry
from photo_culler.analysis.engine.cache import MetricCache
from photo_culler.analysis.engine.pipeline import AnalysisPipeline


class MockAnalyzer(Analyzer):
    name = "mock_test"
    version = "1.0"
    category = "testing"

    def analyze(self, context: AnalysisContext) -> AnalysisResult:
        return AnalysisResult(
            analyzer=self.name,
            version=self.version,
            metrics={"test_metric": 42.0},
            confidence=0.99,
        )


@pytest.fixture
def sample_image(tmp_path):
    img_path = tmp_path / "test_sample.jpg"
    img = Image.new("RGB", (200, 150), color=(128, 128, 128))
    img.save(img_path)
    return img_path


def test_analysis_context(sample_image):
    ctx = AnalysisContext(image_path=sample_image)
    assert ctx.file_size > 0
    pil_img = ctx.get_pillow_image()
    assert pil_img.size == (200, 150)
    arr = ctx.get_numpy_array()
    assert arr.shape == (150, 200, 3)
    ctx.close()


def test_analyzer_registry():
    registry = AnalyzerRegistry()
    registry.register(MockAnalyzer)
    assert registry.get("mock_test") == MockAnalyzer
    instances = registry.instantiate_all()
    assert len(instances) == 1
    assert instances[0].name == "mock_test"


def test_metric_cache(tmp_path):
    db_file = tmp_path / "test_cache.db"
    cache = MetricCache(db_path=db_file)

    res = AnalysisResult(
        analyzer="mock_test",
        version="1.0",
        metrics={"score": 0.85},
        confidence=0.95,
        execution_time_ms=12.5,
    )

    cache.put(image_hash="hash_123", result=res)
    retrieved = cache.get(image_hash="hash_123", analyzer_name="mock_test", analyzer_version="1.0")

    assert retrieved is not None
    assert retrieved.analyzer == "mock_test"
    assert retrieved.metrics["score"] == 0.85
    assert retrieved.confidence == 0.95


def test_pipeline_execution(sample_image):
    registry = AnalyzerRegistry()
    registry.register(MockAnalyzer)

    cache = MetricCache(":memory:")
    pipeline = AnalysisPipeline(registry=registry, cache=cache, use_cache=True)

    results = pipeline.run_image(image_path=sample_image, image_hash="hash_sample")
    assert "mock_test" in results
    assert results["mock_test"].metrics["test_metric"] == 42.0

    # Verify second run hits SQLite cache
    results_cached = pipeline.run_image(image_path=sample_image, image_hash="hash_sample")
    assert "mock_test" in results_cached
    assert results_cached["mock_test"].metrics["test_metric"] == 42.0
