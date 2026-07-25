"""scan command for file crawler and catalog ingestion."""

import json
from pathlib import Path

import typer

from ...catalog.database import Database
from ...catalog.repositories.photo_repository import PhotoRepository
from ...pairing.raw_jpeg_pairer import RawJpegPairer
from ...scanner.directory_scanner import DirectoryScanner
from ...volumes.detector import VolumeDetector
from ..context import CliContext
from ..output import OutputRenderer


def scan_command(
    ctx: typer.Context,
    path: Path = typer.Argument(..., help="Directory path to scan"),
    quick: bool = typer.Option(True, "--quick/--full-hash", help="Use quick sparse hashing vs full SHA256"),
    read_only: bool = typer.Option(True, "--read-only/--write-marker", help="Do not modify destination cards"),
):
    """Scan directory tree, index media files, extract EXIF metadata, and pair RAW/JPEG/Sidecars."""
    cli_ctx: CliContext = ctx.obj or CliContext()
    renderer = OutputRenderer(no_color=cli_ctx.no_color, quiet=cli_ctx.quiet)

    target_dir = path.resolve()
    if not target_dir.exists() or not target_dir.is_dir():
        renderer.error(f"Directory '{target_dir}' does not exist.")
        raise typer.Exit(code=2)

    v_detector = VolumeDetector()
    volume = v_detector.get_or_create_volume_marker(target_dir)

    scanner = DirectoryScanner()
    file_records = list(scanner.scan(target_dir))

    pairer = RawJpegPairer()
    photos = pairer.pair_files(file_records)

    db = Database(db_path=cli_ctx.catalog_path)
    with db.session() as s:
        repo = PhotoRepository(s)
        for p in photos:
            repo.save_photo(p)

    raw_count = sum(1 for p in photos if any(f.role.value == "raw" for f in p.files))
    jpeg_count = sum(1 for p in photos if any(f.role.value == "jpeg" for f in p.files))

    if cli_ctx.output_format == "json":
        out = {
            "volume": volume.label,
            "path": str(target_dir),
            "files_found": len(file_records),
            "photos_indexed": len(photos),
            "raw_files": raw_count,
            "jpeg_files": jpeg_count,
        }
        print(json.dumps(out, indent=2))
    else:
        rows = [
            ["Volume", volume.label],
            ["Path", str(target_dir)],
            ["Files Discovered", len(file_records)],
            ["Logical Photos", len(photos)],
            ["RAW Files", raw_count],
            ["JPEG Files", jpeg_count],
        ]
        renderer.render_table(title="Scan Results", headers=["Property", "Value"], rows=rows)
        renderer.success(f"Indexed {len(photos)} photos into catalog.")
