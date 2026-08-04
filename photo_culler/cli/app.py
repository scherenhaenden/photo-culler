"""Main Typer application entry point for photo-culler CLI."""

from pathlib import Path

import typer

from .commands.analyze_cmd import analyze_command
from .commands.bursts_cmd import bursts_command
from .commands.config_cmd import config_command
from .commands.decisions_cmd import decisions_command
from .commands.desktop_cmd import desktop_app
from .commands.doctor_cmd import doctor_command
from .commands.evaluate_cmd import evaluate_command
from .commands.group_cmd import group_command
from .commands.init_cmd import init_command
from .commands.photos_cmd import photos_command
from .commands.preselect_cmd import preselect_command
from .commands.report_cmd import report_command
from .commands.scan_cmd import scan_command
from .commands.sessions_cmd import sessions_command
from .commands.verify_cmd import verify_command
from .commands.volumes_cmd import volumes_command
from .commands.web_cmd import web_app
from .context import CliContext

app = typer.Typer(
    name="photo-culler",
    help="High performance automated photo culling and modular analysis framework",
    add_completion=False,
)

# Register command handlers
app.command("init")(init_command)
app.command("doctor")(doctor_command)
app.command("scan")(scan_command)
app.command("verify")(verify_command)
app.command("volumes")(volumes_command)
app.command("photos")(photos_command)
app.command("preselect")(preselect_command)
app.command("analyze")(analyze_command)
app.command("evaluate")(evaluate_command)
app.command("group")(group_command)
app.command("bursts")(bursts_command)
app.command("sessions")(sessions_command)
app.command("decisions")(decisions_command)
app.command("report")(report_command)
app.command("config")(config_command)
app.add_typer(web_app, name="web")
app.add_typer(desktop_app, name="desktop")


@app.callback()
def main_callback(
    ctx: typer.Context,
    catalog: Path = typer.Option(Path("catalog.db"), "--catalog", "-c", help="Catalog database path"),
    json_output: bool = typer.Option(False, "--json", help="Format output as JSON"),
    csv_output: bool = typer.Option(False, "--csv", help="Format output as CSV"),
    quiet: bool = typer.Option(False, "--quiet", "-q", help="Suppress output"),
    verbose: bool = typer.Option(False, "--verbose", "-v", help="Verbose logging"),
    dry_run: bool = typer.Option(False, "--dry-run", help="Simulate actions without committing changes"),
    no_color: bool = typer.Option(False, "--no-color", help="Disable colored terminal output"),
):
    """Global configuration callback establishing CliContext."""
    output_format = "human"
    if json_output:
        output_format = "json"
    elif csv_output:
        output_format = "csv"

    ctx.obj = CliContext(
        catalog_path=catalog,
        verbose=verbose,
        quiet=quiet,
        output_format=output_format,
        dry_run=dry_run,
        no_color=no_color,
    )


def main():
    app()


if __name__ == "__main__":
    main()
