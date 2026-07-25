"""report command for generating culling summary and statistics."""

import typer
import json

from ..context import CliContext
from ..output import OutputRenderer
from ...catalog.database import Database
from ...catalog.repositories.photo_repository import PhotoRepository
from ...reports.summary_report import ReportGenerator


def report_command(
    ctx: typer.Context,
    report_type: str = typer.Argument("summary", help="Report type: summary, precull, rejects"),
    format_opt: str = typer.Option("human", "--format", help="Format: human, json, csv"),
):
    """Generate session summary reports and culling metrics."""
    cli_ctx: CliContext = ctx.obj or CliContext()
    renderer = OutputRenderer(no_color=cli_ctx.no_color, quiet=cli_ctx.quiet)

    db = Database(db_path=cli_ctx.catalog_path)
    with db.session() as s:
        repo = PhotoRepository(s)
        photos = repo.list_all()

        generator = ReportGenerator()
        summary = generator.generate_summary(photos)

        if format_opt == "json" or cli_ctx.output_format == "json":
            print(json.dumps(summary, indent=2))
        else:
            rows = [
                ["Total Photos", summary["total_photos"]],
                ["RAW Files", summary["raw_count"]],
                ["JPEG Files", summary["jpeg_count"]],
                ["Kept Photos", summary["culling_summary"]["total_kept"]],
                ["Rejected Photos", summary["culling_summary"]["total_rejected"]],
                ["Keep Rate", f"{summary['culling_summary']['keep_rate_pct']}%"],
            ]
            renderer.render_table(title="Photo Culling Summary Report", headers=["Metric", "Value"], rows=rows)
