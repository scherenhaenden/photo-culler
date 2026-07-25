"""volumes command for managing disk storage devices."""

import typer
from pathlib import Path

from ..context import CliContext
from ..output import OutputRenderer
from ...catalog.database import Database
from ...catalog.schema import VolumeDB


def volumes_command(
    ctx: typer.Context,
    action: str = typer.Argument("list", help="Action: list, inspect, register"),
    path: Path = typer.Option(Path("."), "--path", "-p", help="Target volume directory"),
):
    """List, inspect, and manage known volumes and storage devices."""
    cli_ctx: CliContext = ctx.obj or CliContext()
    renderer = OutputRenderer(no_color=cli_ctx.no_color, quiet=cli_ctx.quiet)

    db = Database(db_path=cli_ctx.catalog_path)
    with db.session() as s:
        vols = s.query(VolumeDB).all()

        rows = []
        for v in vols:
            free_gb = round(v.available_bytes / 1e9, 1)
            total_gb = round(v.total_bytes / 1e9, 1)
            rows.append([v.volume_id[:8], v.label, f"{free_gb} GB / {total_gb} GB", v.mount_point, "ONLINE" if v.is_online else "OFFLINE"])

        renderer.render_table(title="Registered Storage Volumes", headers=["ID", "Label", "Available", "Mount Point", "Status"], rows=rows)
