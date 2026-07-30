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


MIGRATIONS: tuple[tuple[int, Migration], ...] = ((1, _v1_gallery_import),)


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
