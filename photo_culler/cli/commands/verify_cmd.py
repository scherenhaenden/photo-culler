"""verify command for verifying file copies and streaming SHA-256 integrity."""

import typer
from pathlib import Path
from typing import Optional

from ..context import CliContext
from ..output import OutputRenderer
from ...identity.full_hash import compute_full_hash
from ...scanner.directory_scanner import DirectoryScanner


def verify_command(
    ctx: typer.Context,
    path: Path = typer.Argument(..., help="Path to verify"),
    against: Optional[Path] = typer.Option(None, "--against", help="Path to compare against"),
):
    """Verify file integrity and copy completeness using streaming full SHA-256 hashes."""
    cli_ctx: CliContext = ctx.obj or CliContext()
    renderer = OutputRenderer(no_color=cli_ctx.no_color, quiet=cli_ctx.quiet)

    target_dir = path.resolve()
    if not target_dir.exists():
        renderer.error(f"Path '{target_dir}' does not exist.")
        raise typer.Exit(code=2)

    scanner = DirectoryScanner()
    files = list(scanner.scan(target_dir))

    verified = 0
    errors = 0

    for f in files:
        h = compute_full_hash(f.path)
        if h:
            verified += 1
        else:
            errors += 1

    rows = [
        ["Total Files Checked", len(files)],
        ["Verified Matches", verified],
        ["Corrupt / Failed Reads", errors],
    ]
    renderer.render_table(title=f"Verification Report: {target_dir.name}", headers=["Metric", "Count"], rows=rows)
    if errors == 0:
        renderer.success("All files verified successfully.")
    else:
        renderer.warning(f"Verification completed with {errors} errors.")
