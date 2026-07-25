"""config command for managing YAML configuration parameters."""

import typer

from ..context import CliContext
from ..output import OutputRenderer


def config_command(
    ctx: typer.Context,
    action: str = typer.Argument("show", help="Action: show, get, set"),
    key: str = typer.Argument(None, help="Configuration key"),
    value: str = typer.Argument(None, help="Configuration value"),
):
    """View and modify photo-culler environment configuration settings."""
    cli_ctx: CliContext = ctx.obj or CliContext()
    renderer = OutputRenderer(no_color=cli_ctx.no_color, quiet=cli_ctx.quiet)

    config_data = [
        ["catalog.path", str(cli_ctx.catalog_path)],
        ["cache.path", str(cli_ctx.cache_path)],
        ["analysis.workers", "8"],
        ["analysis.default_profile", "fast"],
        ["safety.write_to_camera_cards", "false"],
        ["safety.delete_originals", "false"],
    ]

    renderer.render_table(
        title="System Configuration Settings", headers=["Setting Key", "Current Value"], rows=config_data
    )
