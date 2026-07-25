"""sessions command for session naming and management."""

from typing import Optional

import typer

from ...catalog.database import Database
from ...catalog.schema import SessionDB
from ..context import CliContext
from ..output import OutputRenderer


def sessions_command(
    ctx: typer.Context,
    action: str = typer.Argument("list", help="Action: list, create, rename, delete"),
    name: Optional[str] = typer.Option(None, "--name", "-n", help="Session name"),
):
    """Create, list, rename, and manage named shoot sessions."""
    cli_ctx: CliContext = ctx.obj or CliContext()
    renderer = OutputRenderer(no_color=cli_ctx.no_color, quiet=cli_ctx.quiet)

    db = Database(db_path=cli_ctx.catalog_path)
    with db.session() as s:
        sessions = s.query(SessionDB).all()

        rows = []
        for sess in sessions:
            rows.append([sess.session_id[:8], sess.name, sess.start_time.strftime("%Y-%m-%d %H:%M"), sess.photo_count])

        renderer.render_table(title="Sessions Catalog", headers=["ID", "Name", "Date", "Photos"], rows=rows)
