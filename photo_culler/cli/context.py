"""Global CLI context state."""

from dataclasses import dataclass
from pathlib import Path


@dataclass
class CliContext:
    catalog_path: Path = Path("catalog.db")
    config_path: Path = Path("~/.config/photo-culler/config.yaml").expanduser()
    cache_path: Path = Path("~/.cache/photo-culler").expanduser()
    verbose: bool = False
    quiet: bool = False
    output_format: str = "human"  # human, json, csv, ndjson
    dry_run: bool = False
    no_color: bool = False
