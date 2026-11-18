"""Write a non-destructive JPEG burst preselection manifest."""

import csv
from pathlib import Path

import typer

from photo_culler.selection.preselection import JpegPreselector


def preselect_command(
    jpeg_dir: Path = typer.Argument(..., help="Folder containing the camera JPEGs (non-recursive)."),
    output: Path = typer.Option(Path("preselection.csv"), "--output", "-o", help="CSV manifest to create."),
    raw_dir: Path | None = typer.Option(
        None, "--raw-dir", help="Matching RAW folder; adds the corresponding RAW path to the manifest."
    ),
    duplicate_similarity: float = typer.Option(0.88, "--duplicate-similarity", min=0.5, max=1.0),
):
    """Select one technically strongest JPEG per near-identical burst; never moves or edits photos."""
    if not jpeg_dir.is_dir():
        raise typer.BadParameter(f"Not a directory: {jpeg_dir}")
    paths = sorted(path for path in jpeg_dir.iterdir() if path.is_file() and path.suffix.lower() in {".jpg", ".jpeg"})
    selections = JpegPreselector(duplicate_similarity=duplicate_similarity).select(paths)
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("w", newline="", encoding="utf-8") as handle:
        writer = csv.DictWriter(
            handle,
            fieldnames=["jpeg_path", "raw_path", "group", "selected", "representative_jpeg", "quality", "similarity"],
        )
        writer.writeheader()
        for item in selections:
            writer.writerow({
                "jpeg_path": item.frame.path,
                "raw_path": _matching_raw(raw_dir, item.frame.path.stem) if raw_dir else "",
                "group": item.group_id,
                "selected": "yes" if item.selected else "no",
                "representative_jpeg": item.representative,
                "quality": f"{item.frame.quality:.4f}",
                "similarity": f"{item.similarity_to_representative:.4f}",
            })
    kept = sum(item.selected for item in selections)
    typer.echo(f"Preselection complete: {kept}/{len(selections)} JPEGs selected. Manifest: {output}")


def _matching_raw(raw_dir: Path, stem: str) -> str:
    """Resolve common RAW suffixes without claiming a pair when it is absent."""
    for suffix in (".NEF", ".nef", ".ARW", ".arw", ".CR3", ".cr3", ".DNG", ".dng"):
        candidate = raw_dir / f"{stem}{suffix}"
        if candidate.is_file():
            return str(candidate)
    return ""
