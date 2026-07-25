"""web CLI subcommand."""

import webbrowser
import typer
import uvicorn
from rich.console import Console
from photo_culler.cli.context import get_cli_context
from photo_culler.web.app import create_app

web_app = typer.Typer(help="Launch local FastAPI Web UI application.")
console = Console()


@web_app.callback(invoke_without_command=True)
def main(
    ctx: typer.Context,
    host: str = typer.Option("127.0.0.1", "--host", "-h", help="Host interface to bind (default: 127.0.0.1)"),
    port: int = typer.Option(8765, "--port", "-p", help="Port to listen on (default: 8765)"),
    open_browser: bool = typer.Option(True, "--open/--no-open", help="Automatically open web browser"),
):
    """Start local web server and serve photo-culler UI."""
    cli_ctx = get_cli_context(ctx)
    app = create_app(catalog_path=cli_ctx.catalog_path)

    url = f"http://{host}:{port}"
    console.print(f"\n[bold green]Photo Culler Web UI Running[/bold green]\nURL: [bold underline blue]{url}[/bold underline blue]\nCatalog: {cli_ctx.catalog_path}\n")

    if open_browser:
        webbrowser.open(url)

    uvicorn.run(app, host=host, port=port, log_level="info")
