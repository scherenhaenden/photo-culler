"""decisions command for setting human culling flags."""

from typing import Optional

import typer

from ...catalog.database import Database
from ...catalog.repositories.photo_repository import PhotoRepository
from ...core.enums import DecisionState
from ..context import CliContext
from ..helpers.photo_selector import PhotoSelector
from ..output import OutputRenderer


def decisions_command(
    ctx: typer.Context,
    action: str = typer.Argument("set", help="Action: set, list"),
    decision: str = typer.Option(
        ..., "--decision", "-d", help="Decision: keep, best, alternate, review, reject_technical, reject_redundant"
    ),
    photo_id: Optional[str] = typer.Option(None, "--photo-id", help="Photo ID"),
    session: Optional[str] = typer.Option(None, "--session", help="Session filter"),
    confirm: bool = typer.Option(False, "--confirm", help="Confirm batch decision updates"),
):
    """Set or update culling decision states on selected photos."""
    cli_ctx: CliContext = ctx.obj or CliContext()
    renderer = OutputRenderer(no_color=cli_ctx.no_color, quiet=cli_ctx.quiet)

    try:
        new_state = DecisionState(decision.upper())
    except ValueError:
        renderer.error(f"Invalid decision state: '{decision}'. Allowed: {[d.value for d in DecisionState]}")
        raise typer.Exit(code=2)

    db = Database(db_path=cli_ctx.catalog_path)
    with db.session() as s:
        repo = PhotoRepository(s)
        selector = PhotoSelector(repo)
        photos = selector.resolve(photo_id=photo_id, session=session)

        if not photos:
            renderer.warning("No photos matched target criteria.")
            return

        if len(photos) > 5 and not confirm:
            renderer.warning(f"Batch operation affects {len(photos)} photos. Add '--confirm' to execute.")
            return

        for p in photos:
            p.decision = new_state
            repo.save_photo(p)

        renderer.success(f"Updated {len(photos)} photos to decision '{new_state.value}'.")
