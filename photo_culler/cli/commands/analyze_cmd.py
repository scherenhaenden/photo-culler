"""analyze command for technical image measurement pipeline."""

from pathlib import Path
from typing import Optional

import typer

import photo_culler.analysis.analyzers.technical  # noqa

from ...analysis.engine.cache import MetricCache
from ...analysis.engine.pipeline import AnalysisPipeline
from ...catalog.database import Database
from ...catalog.repositories.photo_repository import PhotoRepository
from ...scoring.technical_score import TechnicalScorer
from ...selection.decisions.rules import SelectionRulesEngine
from ..context import CliContext
from ..helpers.asset_resolver import AnalysisAssetResolver
from ..helpers.photo_selector import PhotoSelector
from ..output import OutputRenderer


def analyze_command(
    ctx: typer.Context,
    path: Optional[Path] = typer.Argument(None, help="Optional path to analyze"),
    session: Optional[str] = typer.Option(None, "--session", "-s", help="Filter by session"),
    profile: str = typer.Option("fast", "--profile", help="Profile: fast, technical, deep"),
    status: Optional[str] = typer.Option(None, "--status", help="Filter by status (e.g. pending)"),
):
    """Run technical measurement analyzers on selected photos using PhotoSelector and AnalysisAssetResolver."""
    cli_ctx: CliContext = ctx.obj or CliContext()
    renderer = OutputRenderer(no_color=cli_ctx.no_color, quiet=cli_ctx.quiet)

    db = Database(db_path=cli_ctx.catalog_path)
    with db.session() as s:
        repo = PhotoRepository(s)
        selector = PhotoSelector(repo)
        photos = selector.resolve(path=path, session=session, status=status)

        if not photos:
            renderer.warning("No photos matched selection criteria for analysis.")
            return

        cache = MetricCache(db_path=str(cli_ctx.catalog_path) + ".metrics.db")
        pipeline = AnalysisPipeline(cache=cache, use_cache=True)
        asset_resolver = AnalysisAssetResolver()
        scorer = TechnicalScorer()
        rules_engine = SelectionRulesEngine()

        renderer.print(
            f"Analyzing [bold cyan]{len(photos)}[/bold cyan] photos (Profile: [yellow]{profile}[/yellow])..."
        )

        for i, p in enumerate(photos, start=1):
            img_asset = asset_resolver.resolve(p, prefer_jpeg=True)
            if not img_asset or not img_asset.exists():
                continue

            results = pipeline.run_image(image_path=img_asset, image_hash=p.photo_id)
            tech_score = scorer.calculate_score(results)

            p.score = tech_score["final_score"]
            p.quality_tier = tech_score["quality_tier"]

        rules_engine.apply_decisions(photos)
        for p in photos:
            repo.save_photo(p)

        renderer.success(f"Successfully analyzed {len(photos)} photos.")
