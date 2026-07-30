"""Global CLI context state."""

from dataclasses import dataclass
from pathlib import Path

import typer


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


def get_cli_context(ctx: typer.Context) -> CliContext:
    """Retrieve or initialize global CliContext from Typer Context."""
    if hasattr(ctx, "obj") and isinstance(ctx.obj, CliContext):
        return ctx.obj
    return CliContext()
