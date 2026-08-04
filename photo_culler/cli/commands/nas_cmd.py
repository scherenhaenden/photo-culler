"""nas CLI subcommand."""

import os
import typer
import uvicorn
from rich.console import Console

from photo_culler.cli.context import get_cli_context
from photo_culler.web.app import create_app

nas_app = typer.Typer(help="Launch headless NAS server with dynamic thermal throttling.")
console = Console()


@nas_app.callback(invoke_without_command=True)
def main(
    ctx: typer.Context,
    host: str = typer.Option("0.0.0.0", "--host", "-h", help="Host interface to bind (default: 0.0.0.0 for NAS)"),
    port: int = typer.Option(8765, "--port", "-p", help="Port to listen on (default: 8765)"),
    high_temp: float = typer.Option(75.0, "--high-temp", help="Temperature threshold in °C to pause active analysis"),
    low_temp: float = typer.Option(60.0, "--low-temp", help="Temperature threshold in °C to resume analysis"),
    interval: float = typer.Option(5.0, "--interval", help="Polling interval in seconds"),
):
    """Start headless NAS server with thermal throttling to prevent overheating."""
    cli_ctx = get_cli_context(ctx)

    if low_temp >= high_temp:
        console.print(
            "[bold red]Error: low-temp threshold must be strictly lower than high-temp threshold.[/bold red]"
        )
        raise typer.Exit(code=1)

    # Inject config via environment variables, which will be picked up by create_app()'s env overrides!
    os.environ["PHOTO_CULLER_NAS_MONITOR"] = "1"
    os.environ["PHOTO_CULLER_NAS_MAX_TEMP"] = str(high_temp)
    os.environ["PHOTO_CULLER_NAS_MIN_TEMP"] = str(low_temp)
    os.environ["PHOTO_CULLER_NAS_INTERVAL"] = str(interval)

    app = create_app(catalog_path=cli_ctx.catalog_path)

    url = f"http://{host}:{port}"
    console.print(
        f"\n[bold green]Photo Culler NAS Server Running[/bold green]\n"
        f"URL: [bold underline blue]{url}[/bold underline blue]\n"
        f"Catalog: {cli_ctx.catalog_path}\n"
        f"Thermal Protection: [bold yellow]ENABLED[/bold yellow]\n"
        f"High Limit: [bold red]{high_temp}°C[/bold red] | Low Resume: [bold green]{low_temp}°C[/bold green]\n"
        f"Interval: {interval}s\n"
    )

    uvicorn.run(app, host=host, port=port, log_level="info")
