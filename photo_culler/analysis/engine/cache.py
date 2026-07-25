"""Persistent SQLite caching layer for intermediate analyzer measurements."""

import sqlite3
import json
import time
from pathlib import Path
from typing import Dict, Any, Optional, Union, List

from .result import AnalysisResult


class MetricCache:
    """SQLite metric store to preserve all raw intermediate analyzer outputs.
    
    Prevents costly re-computation of image metrics when scoring rules,
    weights, or algorithms are modified.
    """

    def __init__(self, db_path: Union[str, Path] = ":memory:"):
        self.db_path = str(db_path)
        self._shared_conn: Optional[sqlite3.Connection] = None
        if self.db_path != ":memory:":
            Path(self.db_path).parent.mkdir(parents=True, exist_ok=True)
        else:
            self._shared_conn = sqlite3.connect(":memory:")
            self._shared_conn.row_factory = sqlite3.Row
        self._init_db()

    def _get_connection(self) -> sqlite3.Connection:
        if self.db_path == ":memory:":
            return self._shared_conn
        conn = sqlite3.connect(self.db_path)
        conn.row_factory = sqlite3.Row
        return conn

    def _init_db(self):
        conn = self._get_connection()
        conn.execute("""
            CREATE TABLE IF NOT EXISTS analyzer_metrics (
                image_hash TEXT NOT NULL,
                analyzer_name TEXT NOT NULL,
                analyzer_version TEXT NOT NULL,
                metrics_json TEXT NOT NULL,
                confidence REAL NOT NULL,
                execution_time_ms REAL NOT NULL,
                created_at REAL NOT NULL,
                PRIMARY KEY (image_hash, analyzer_name, analyzer_version)
            )
        """)
        conn.execute("""
            CREATE INDEX IF NOT EXISTS idx_metrics_hash 
            ON analyzer_metrics (image_hash)
        """)
        conn.commit()
        if self.db_path != ":memory:":
            conn.close()

    def get(self, image_hash: str, analyzer_name: str, analyzer_version: str) -> Optional[AnalysisResult]:
        """Retrieve stored AnalysisResult if matching hash, name, and version exist."""
        conn = self._get_connection()
        try:
            cursor = conn.execute(
                """
                SELECT analyzer_name, analyzer_version, metrics_json, confidence, execution_time_ms
                FROM analyzer_metrics
                WHERE image_hash = ? AND analyzer_name = ? AND analyzer_version = ?
                """,
                (image_hash, analyzer_name, analyzer_version)
            )
            row = cursor.fetchone()
            if not row:
                return None
            
            metrics = json.loads(row["metrics_json"])
            return AnalysisResult(
                analyzer=row["analyzer_name"],
                version=row["analyzer_version"],
                metrics=metrics,
                confidence=float(row["confidence"]),
                error=None,
                execution_time_ms=float(row["execution_time_ms"]),
            )
        finally:
            if self.db_path != ":memory:":
                conn.close()

    def get_all_for_image(self, image_hash: str) -> List[AnalysisResult]:
        """Retrieve all analyzer results cached for a given image hash."""
        conn = self._get_connection()
        try:
            cursor = conn.execute(
                """
                SELECT analyzer_name, analyzer_version, metrics_json, confidence, execution_time_ms
                FROM analyzer_metrics
                WHERE image_hash = ?
                """,
                (image_hash,)
            )
            results = []
            for row in cursor.fetchall():
                results.append(AnalysisResult(
                    analyzer=row["analyzer_name"],
                    version=row["analyzer_version"],
                    metrics=json.loads(row["metrics_json"]),
                    confidence=float(row["confidence"]),
                    error=None,
                    execution_time_ms=float(row["execution_time_ms"]),
                ))
            return results
        finally:
            if self.db_path != ":memory:":
                conn.close()

    def put(self, image_hash: str, result: AnalysisResult):
        """Store or update AnalysisResult metrics in SQLite database."""
        if result.error:
            # Do not cache failed runs
            return

        conn = self._get_connection()
        try:
            conn.execute(
                """
                INSERT INTO analyzer_metrics 
                (image_hash, analyzer_name, analyzer_version, metrics_json, confidence, execution_time_ms, created_at)
                VALUES (?, ?, ?, ?, ?, ?, ?)
                ON CONFLICT(image_hash, analyzer_name, analyzer_version) DO UPDATE SET
                    metrics_json = excluded.metrics_json,
                    confidence = excluded.confidence,
                    execution_time_ms = excluded.execution_time_ms,
                    created_at = excluded.created_at
                """,
                (
                    image_hash,
                    result.analyzer,
                    result.version,
                    json.dumps(result.metrics),
                    result.confidence,
                    result.execution_time_ms,
                    time.time(),
                )
            )
            conn.commit()
        finally:
            if self.db_path != ":memory:":
                conn.close()
