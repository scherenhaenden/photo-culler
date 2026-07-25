"""desktop CLI subcommand."""

import typer
from photo_culler.cli.context import get_cli_context
from photo_culler.desktop.app import run_desktop

desktop_app = typer.Typer(help="Launch native pywebview desktop window application.")


@desktop_app.callback(invoke_without_command=True)
def main(
    ctx: typer.Context,
    fullscreen: bool = typer.Option(False, "--fullscreen", "-f", help="Launch in fullscreen mode"),
):
    """Start native Desktop window for photo-culler."""
    cli_ctx = get_cli_context(ctx)
    cli_ctx.console.print_panel(
        f"[bold blue]Launching Photo Culler Desktop Window...[/bold blue]\nCatalog: [dim]{cli_ctx.catalog_path}[/dim]",
        title="Desktop Mode",
    )
    run_desktop(catalog_path=cli_ctx.catalog_path, fullscreen=fullscreen)
