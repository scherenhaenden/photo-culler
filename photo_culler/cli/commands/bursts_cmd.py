"""bursts command for burst sequence detection."""

import typer

from ...bursts.temporal_bursts import BurstDetector
from ...catalog.database import Database
from ...catalog.repositories.photo_repository import PhotoRepository
from ..context import CliContext
from ..output import OutputRenderer


def bursts_command(
    ctx: typer.Context,
    action: str = typer.Argument("detect", help="Action: detect, list, show"),
    gap: float = typer.Option(1.5, "--maximum-gap", help="Maximum time gap in seconds for burst clustering"),
):
    """Detect and manage high-speed photo burst sequences."""
    cli_ctx: CliContext = ctx.obj or CliContext()
    renderer = OutputRenderer(no_color=cli_ctx.no_color, quiet=cli_ctx.quiet)

    db = Database(db_path=cli_ctx.catalog_path)
    with db.session() as s:
        repo = PhotoRepository(s)
        photos = repo.list_all()

        detector = BurstDetector(max_burst_gap_seconds=gap)
        bursts = detector.detect_bursts(photos)

        for p in photos:
            repo.save_photo(p)

        rows = []
        for b in bursts:
            rows.append(
                [b.burst_id[:8], len(b.photos), b.representative_photo_id[:8] if b.representative_photo_id else "-"]
            )

        renderer.render_table(
            title="Detected Burst Sequences", headers=["Burst ID", "Photo Count", "Representative ID"], rows=rows
        )
        renderer.success(f"Detected {len(bursts)} burst sequences.")
