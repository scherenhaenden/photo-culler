"""doctor command for environment and system diagnostics."""

import typer
import sys
import shutil
import platform
from pathlib import Path

from ..context import CliContext
from ..output import OutputRenderer
from ...catalog.database import Database


def doctor_command(
    ctx: typer.Context,
    fix: bool = typer.Option(False, "--fix", help="Automatically attempt safe repairs"),
):
    """Run diagnostics on environment, SQLite database, dependencies, and external tools."""
    cli_ctx: CliContext = ctx.obj or CliContext()
    renderer = OutputRenderer(no_color=cli_ctx.no_color, quiet=cli_ctx.quiet)

    # 1. Environment checks
    py_version = sys.version.split()[0]
    os_info = f"{platform.system()} {platform.release()}"

    # 2. Tools
    exiftool = shutil.which("exiftool") is not None
    vips = shutil.which("vips") is not None
    rawtherapee = shutil.which("rawtherapee") is not None

    # 3. Database
    db_ok = True
    try:
        db = Database(db_path=cli_ctx.catalog_path)
        with db.session() as s:
            s.execute(__import__("sqlalchemy").text("SELECT 1"))
    except Exception:
        db_ok = False

    rows = [
        ["Python Version", py_version, "OK" if sys.version_info >= (3, 9) else "WARN"],
        ["OS Platform", os_info, "OK"],
        ["Catalog Database", str(cli_ctx.catalog_path), "OK" if db_ok else "ERROR"],
        ["ExifTool", "Installed" if exiftool else "Not found (Fallback PIL active)", "INFO"],
        ["libvips", "Installed" if vips else "Not found (Fallback PIL active)", "INFO"],
        ["RawTherapee", "Installed" if rawtherapee else "Not found", "INFO"],
    ]

    renderer.render_table(title="Photo Culler Environment Diagnostics", headers=["Component", "Details", "Status"], rows=rows)
