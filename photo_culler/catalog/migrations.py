"""Small versioned migration runner for the local SQLite catalog.

The project intentionally keeps migrations explicit rather than treating
``metadata.create_all`` as schema evolution. PostgreSQL remains unsupported by
this runner until its operational contract is implemented and tested.
"""

from collections.abc import Callable

from sqlalchemy import Connection, inspect, text

Migration = Callable[[Connection], None]


def _v1_gallery_import(connection: Connection) -> None:
    """Add gallery ownership to catalogs created before gallery support."""
    columns = {column["name"] for column in inspect(connection).get_columns("photos")}
    if "gallery_id" not in columns:
        connection.execute(text("ALTER TABLE photos ADD COLUMN gallery_id VARCHAR(36)"))
    connection.execute(text("CREATE INDEX IF NOT EXISTS ix_photos_gallery_id ON photos (gallery_id)"))


def _v2_import_pause_resume(connection: Connection) -> None:
    """Add durable cooperative pause state to existing import catalogs."""
    columns = {column["name"] for column in inspect(connection).get_columns("import_jobs")}
    if "pause_requested" not in columns:
        connection.execute(text("ALTER TABLE import_jobs ADD COLUMN pause_requested BOOLEAN NOT NULL DEFAULT 0"))
    if "resume_state" not in columns:
        connection.execute(text("ALTER TABLE import_jobs ADD COLUMN resume_state VARCHAR(32)"))


def _add_column(connection: Connection, table: str, name: str, definition: str) -> None:
    """Add one nullable/defaulted column when upgrading an existing SQLite catalog."""
    columns = {column["name"] for column in inspect(connection).get_columns(table)}
    if name not in columns:
        connection.execute(text(f"ALTER TABLE {table} ADD COLUMN {name} {definition}"))


def _v3_scan_reconciliation(connection: Connection) -> None:
    """Add source/revision state used to reconcile rescans and offline volumes."""
    _add_column(connection, "import_sources", "status", "VARCHAR(32) NOT NULL DEFAULT 'online'")
    _add_column(connection, "import_sources", "last_seen_at", "DATETIME")
    _add_column(connection, "import_jobs", "scan_revision_id", "VARCHAR(36)")
    _add_column(connection, "files", "import_source_id", "VARCHAR(36)")
    _add_column(connection, "files", "last_seen_revision_id", "VARCHAR(36)")
    _add_column(connection, "files", "source_relative_path", "VARCHAR(1024)")
    _add_column(connection, "files", "status", "VARCHAR(32) NOT NULL DEFAULT 'present'")
    connection.execute(text("CREATE INDEX IF NOT EXISTS ix_import_sources_status ON import_sources (status)"))
    connection.execute(
        text("CREATE INDEX IF NOT EXISTS ix_import_jobs_scan_revision_id ON import_jobs (scan_revision_id)")
    )
    connection.execute(text("CREATE INDEX IF NOT EXISTS ix_files_import_source_id ON files (import_source_id)"))
    connection.execute(
        text("CREATE INDEX IF NOT EXISTS ix_files_last_seen_revision_id ON files (last_seen_revision_id)")
    )
    connection.execute(text("CREATE INDEX IF NOT EXISTS ix_files_status ON files (status)"))


def _v4_import_exclusions(connection: Connection) -> None:
    """Persist source exclusion patterns for repeatable rescans."""
    _add_column(connection, "import_sources", "exclude_patterns", "TEXT NOT NULL DEFAULT '[]'")


def _v5_moved_file_tracking(connection: Connection) -> None:
    """Record unambiguous quick-hash move matches per scan revision."""
    _add_column(connection, "scan_revisions", "moved_files", "INTEGER NOT NULL DEFAULT 0")


def _v6_edit_documents(connection: Connection) -> None:
    """Create versioned non-destructive edit recipe storage."""
    connection.execute(
        text(
            "CREATE TABLE IF NOT EXISTS edit_documents ("
            "id VARCHAR(36) PRIMARY KEY, "
            "photo_id INTEGER NOT NULL UNIQUE REFERENCES photos(id), "
            "contract_version INTEGER NOT NULL DEFAULT 1, "
            "revision INTEGER NOT NULL DEFAULT 0, "
            "recipe_json TEXT NOT NULL DEFAULT '{}', "
            "undo_stack_json TEXT NOT NULL DEFAULT '[]', "
            "redo_stack_json TEXT NOT NULL DEFAULT '[]', "
            "created_at DATETIME NOT NULL, "
            "updated_at DATETIME NOT NULL"
            ")"
        )
    )
    connection.execute(
        text("CREATE UNIQUE INDEX IF NOT EXISTS ix_edit_documents_photo_id ON edit_documents (photo_id)")
    )


def _v7_analysis_explanations(connection: Connection) -> None:
    """Store the explanation that produced each photo's technical score."""
    _add_column(connection, "photos", "analysis_summary_json", "TEXT NOT NULL DEFAULT '{}'")


MIGRATIONS: tuple[tuple[int, Migration], ...] = (
    (1, _v1_gallery_import),
    (2, _v2_import_pause_resume),
    (3, _v3_scan_reconciliation),
    (4, _v4_import_exclusions),
    (5, _v5_moved_file_tracking),
    (6, _v6_edit_documents),
    (7, _v7_analysis_explanations),
)


def migrate(connection: Connection) -> int:
    """Apply pending migrations and return the resulting schema version."""
    connection.execute(
        text(
            "CREATE TABLE IF NOT EXISTS schema_migrations "
            "(version INTEGER PRIMARY KEY, applied_at VARCHAR(40) NOT NULL)"
        )
    )
    applied = {row[0] for row in connection.execute(text("SELECT version FROM schema_migrations"))}
    for version, migration in MIGRATIONS:
        if version in applied:
            continue
        migration(connection)
        connection.execute(
            text("INSERT INTO schema_migrations(version, applied_at) VALUES (:version, CURRENT_TIMESTAMP)"),
            {"version": version},
        )
        applied.add(version)
    return max(applied, default=0)
