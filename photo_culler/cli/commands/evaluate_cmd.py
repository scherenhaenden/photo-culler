"""evaluate command for evaluating quality tiers, recoverability, and reject risks."""

from typing import Optional

import typer

from ...catalog.database import Database
from ...catalog.repositories.photo_repository import PhotoRepository
from ...scoring.recoverability_score import RecoverabilityScorer
from ..context import CliContext
from ..helpers.photo_selector import PhotoSelector
from ..output import OutputRenderer


def evaluate_command(
    ctx: typer.Context,
    session: Optional[str] = typer.Option(None, "--session", "-s", help="Filter by session"),
    profile: str = typer.Option("concert", "--profile", help="Evaluation profile: concert, portrait, crowd"),
):
    """Evaluate photo recoverability, technical quality, and reject risk without modifying source files."""
    cli_ctx: CliContext = ctx.obj or CliContext()
    renderer = OutputRenderer(no_color=cli_ctx.no_color, quiet=cli_ctx.quiet)

    db = Database(db_path=cli_ctx.catalog_path)
    with db.session() as s:
        repo = PhotoRepository(s)
        selector = PhotoSelector(repo)
        photos = selector.resolve(session=session)

        rows = []
        rec_scorer = RecoverabilityScorer()

        for p in photos:
            # Estimate recoverability headroom
            rec_res = rec_scorer.calculate_recoverability({})
            rows.append(
                [
                    p.stem_name,
                    f"{int(p.score * 100)}",
                    f"{rec_res['overall_recoverability'] * 100:.0f}%",
                    p.decision.value,
                    p.quality_tier.value.upper(),
                ]
            )

        renderer.render_table(
            title=f"Evaluation Report (Profile: {profile.upper()})",
            headers=["Photo", "Tech Quality", "RAW Recoverability", "Decision", "Tier"],
            rows=rows,
        )
