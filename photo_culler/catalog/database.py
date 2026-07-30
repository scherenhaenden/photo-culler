"""Database connection and session factory for photo-culler catalog."""

from contextlib import contextmanager
from pathlib import Path
from typing import Generator, Union

from sqlalchemy import create_engine
from sqlalchemy.orm import Session, sessionmaker

from .schema import Base


class Database:
    """Catalog database manager handling SQLite initialization and sessions."""

    def __init__(self, db_path: Union[str, Path] = "catalog.db"):
        self.db_path = str(db_path)
        if self.db_path != ":memory:":
            Path(self.db_path).parent.mkdir(parents=True, exist_ok=True)
            db_uri = f"sqlite:///{self.db_path}"
        else:
            db_uri = "sqlite:///:memory:"

        self.engine = create_engine(db_uri, connect_args={"check_same_thread": False}, echo=False)

        # Configure WAL mode and synchronous settings on connect
        from sqlalchemy import event
        @event.listens_for(self.engine, "connect")
        def set_sqlite_pragma(dbapi_connection, connection_record):
            cursor = dbapi_connection.cursor()
            try:
                cursor.execute("PRAGMA journal_mode=WAL;")
                cursor.execute("PRAGMA synchronous=NORMAL;")
                cursor.execute("PRAGMA busy_timeout=5000;")
            except Exception:
                pass
            finally:
                cursor.close()

        self.SessionLocal = sessionmaker(autocommit=False, autoflush=False, bind=self.engine)
        self.create_tables()

    def create_tables(self):
        """Create database tables if they do not exist."""
        Base.metadata.create_all(bind=self.engine)

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
