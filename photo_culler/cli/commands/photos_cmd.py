"""photos command for listing and searching logical photo entries."""

import json
from pathlib import Path
from typing import Optional

import typer

from ...catalog.database import Database
from ...catalog.repositories.photo_repository import PhotoRepository
from ..context import CliContext
from ..helpers.photo_selector import PhotoSelector
from ..output import OutputRenderer


def photos_command(
    ctx: typer.Context,
    session: Optional[str] = typer.Option(None, "--session", "-s", help="Filter by session name/ID"),
    decision: Optional[str] = typer.Option(None, "--decision", "-d", help="Filter by decision state"),
    photo_id: Optional[str] = typer.Option(None, "--photo-id", help="Filter by exact photo ID"),
    path: Optional[Path] = typer.Option(None, "--path", "-p", help="Filter by path"),
):
    """List and inspect logical photo entries in catalog."""
    cli_ctx: CliContext = ctx.obj or CliContext()
    renderer = OutputRenderer(no_color=cli_ctx.no_color, quiet=cli_ctx.quiet)

    db = Database(db_path=cli_ctx.catalog_path)
    with db.session() as s:
        repo = PhotoRepository(s)
        selector = PhotoSelector(repo)
        photos = selector.resolve(path=path, session=session, decision=decision, photo_id=photo_id)

        if cli_ctx.output_format == "json":
            out = [
                {
                    "photo_id": p.photo_id,
                    "stem": p.stem_name,
                    "score": p.score,
                    "decision": p.decision.value,
                    "tier": p.quality_tier.value,
                    "subfile_count": len(p.files),
                }
                for p in photos
            ]
            print(json.dumps(out, indent=2))
        else:
            rows = []
            for p in photos[:100]:  # Limit display to 100 rows in terminal
                dt_str = (
                    p.metadata.capture_time.strftime("%Y-%m-%d %H:%M:%S")
                    if p.metadata and p.metadata.capture_time
                    else "-"
                )
                cam_str = f"{p.metadata.camera_model}" if p.metadata and p.metadata.camera_model else "-"
                rows.append(
                    [
                        p.photo_id[:8],
                        p.stem_name,
                        dt_str,
                        cam_str,
                        p.decision.value,
                        f"{p.score:.2f}",
                        p.quality_tier.value.upper(),
                    ]
                )

            renderer.render_table(
                title=f"Photos in Catalog ({len(photos)} matched)",
                headers=["ID", "Stem", "Captured", "Camera", "Decision", "Score", "Tier"],
                rows=rows,
            )
