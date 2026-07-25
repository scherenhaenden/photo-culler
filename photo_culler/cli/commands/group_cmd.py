"""group command for timeline clustering."""

import typer

from ..context import CliContext
from ..output import OutputRenderer
from ...catalog.database import Database
from ...catalog.repositories.photo_repository import PhotoRepository
from ...grouping.timeline import SessionDetector


def group_command(
    ctx: typer.Context,
    gap: float = typer.Option(15.0, "--maximum-gap", help="Maximum time gap in minutes for session clustering"),
):
    """Cluster indexed photos chronologically into shoot sessions."""
    cli_ctx: CliContext = ctx.obj or CliContext()
    renderer = OutputRenderer(no_color=cli_ctx.no_color, quiet=cli_ctx.quiet)

    db = Database(db_path=cli_ctx.catalog_path)
    with db.session() as s:
        repo = PhotoRepository(s)
        photos = repo.list_all()

        detector = SessionDetector(max_gap_minutes=gap)
        sessions = detector.detect_sessions(photos)

        for p in photos:
            repo.save_photo(p)

        rows = []
        for sess in sessions:
            rows.append([sess.session_id[:8], sess.name, sess.start_time.strftime("%H:%M:%S"), sess.end_time.strftime("%H:%M:%S"), sess.photo_count])

        renderer.render_table(title="Timeline Shoot Sessions", headers=["Session ID", "Name", "Start Time", "End Time", "Photos"], rows=rows)
        renderer.success(f"Grouped {len(photos)} photos into {len(sessions)} sessions.")
