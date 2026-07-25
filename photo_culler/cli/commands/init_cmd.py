"""init command for catalog and environment setup."""

from pathlib import Path

import typer

from ...catalog.database import Database
from ..context import CliContext
from ..output import OutputRenderer


def init_command(
    ctx: typer.Context,
    catalog: Path = typer.Option(Path("catalog.db"), "--catalog", "-c", help="Catalog database path"),
    force: bool = typer.Option(False, "--force", "-f", help="Force complete initialization without deleting data"),
):
    """Initialize photo-culler catalog database and configuration environment."""
    cli_ctx: CliContext = ctx.obj or CliContext()
    cli_ctx.catalog_path = catalog
    renderer = OutputRenderer(no_color=cli_ctx.no_color, quiet=cli_ctx.quiet)

    db = Database(db_path=catalog)
    db.create_tables()

    content = (
        f"Catalog Database: {catalog.resolve()}\n"
        f"Cache Directory:  {cli_ctx.cache_path}\n"
        f"Config Directory: {cli_ctx.config_path}\n\n"
        "[green]✔ Tables & SQLite schemas ready.[/green]"
    )
    renderer.panel(content, title="Photo Culler Initialized")
