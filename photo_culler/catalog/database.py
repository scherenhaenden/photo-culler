"""Database connection and session factory for photo-culler catalog."""

import os
from contextlib import contextmanager
from dataclasses import dataclass
from enum import Enum
from pathlib import Path
from typing import Generator, Optional, Union

from sqlalchemy import create_engine, event
from sqlalchemy.engine import make_url
from sqlalchemy.orm import Session, sessionmaker

from .migrations import migrate
from .schema import Base


class CatalogBackend(str, Enum):
    """Catalog backends understood by the persistence boundary."""

    SQLITE = "sqlite"
    POSTGRESQL = "postgresql"


@dataclass(frozen=True)
class CatalogConfig:
    """Resolved SQLAlchemy catalog configuration."""

    url: str
    backend: CatalogBackend

    @classmethod
    def resolve(
        cls,
        db_path: Union[str, Path] = "catalog.db",
        db_url: Optional[str] = None,
    ) -> "CatalogConfig":
        configured_url = db_url or os.getenv("PHOTO_CULLER_DATABASE_URL")
        if configured_url:
            driver = make_url(configured_url).get_backend_name()
            if driver == "sqlite":
                backend = CatalogBackend.SQLITE
            elif driver == "postgresql":
                backend = CatalogBackend.POSTGRESQL
            else:
                raise ValueError(f"Unsupported catalog backend: {driver}")
            return cls(url=configured_url, backend=backend)

        path = str(db_path)
        url = "sqlite:///:memory:" if path == ":memory:" else f"sqlite:///{Path(path).resolve()}"
        return cls(url=url, backend=CatalogBackend.SQLITE)


class Database:
    """Catalog database manager with SQLite defaults and a portable SQLAlchemy boundary."""

    def __init__(self, db_path: Union[str, Path] = "catalog.db", db_url: Optional[str] = None):
        self.config = CatalogConfig.resolve(db_path=db_path, db_url=db_url)
        parsed_url = make_url(self.config.url)
        sqlite_database = parsed_url.database if self.config.backend == CatalogBackend.SQLITE else None
        self.db_path = sqlite_database or str(db_path)

        if sqlite_database and sqlite_database != ":memory:":
            Path(sqlite_database).parent.mkdir(parents=True, exist_ok=True)

        connect_args = {"check_same_thread": False} if self.config.backend == CatalogBackend.SQLITE else {}
        self.engine = create_engine(self.config.url, connect_args=connect_args, echo=False)

        if self.config.backend == CatalogBackend.SQLITE:

            @event.listens_for(self.engine, "connect")
            def set_sqlite_pragma(dbapi_connection, connection_record):
                cursor = dbapi_connection.cursor()
                try:
                    cursor.execute("PRAGMA journal_mode=WAL;")
                    cursor.execute("PRAGMA synchronous=NORMAL;")
                    cursor.execute("PRAGMA busy_timeout=5000;")
                finally:
                    cursor.close()

        self.SessionLocal = sessionmaker(autocommit=False, autoflush=False, bind=self.engine)
        self.create_tables()

    def create_tables(self):
        """Create a fresh schema and run versioned upgrades for existing catalogs."""
        Base.metadata.create_all(bind=self.engine)
        with self.engine.begin() as connection:
            migrate(connection)

    @contextmanager
    def session(self) -> Generator[Session, None, None]:
        """Provide a transactional scope around a series of operations."""
        session = self.SessionLocal()
        try:
            yield session
            session.commit()
        except Exception:
            session.rollback()
            raise
        finally:
            session.close()
