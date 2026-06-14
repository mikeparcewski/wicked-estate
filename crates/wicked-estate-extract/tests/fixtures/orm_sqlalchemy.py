"""SQLAlchemy ORM fixture — covers 1.x Column() and 2.0 mapped_column() / relationship()."""
from sqlalchemy import Column, Integer, String, ForeignKey, Text
from sqlalchemy.orm import DeclarativeBase, relationship, mapped_column

SCHEMA_VERSION = 1
DEFAULT_POOL = "default"


class Base(DeclarativeBase):
    pass


class User(Base):
    """SQLAlchemy 1.x style: Column() assignments."""

    __tablename__ = "users"

    id = Column(Integer, primary_key=True)
    username = Column(String(50), nullable=False, unique=True)
    email = Column(String(200), nullable=False)
    bio = Column(Text, nullable=True)
    posts = relationship("Post", back_populates="author")


class Post(Base):
    """SQLAlchemy 2.0 style: mapped_column() assignments."""

    __tablename__ = "posts"

    id: int = mapped_column(Integer, primary_key=True)
    title: str = mapped_column(String(200), nullable=False)
    body: str = mapped_column(Text)
    author_id: int = mapped_column(Integer, ForeignKey("users.id"))
    author = relationship("User", back_populates="posts")


def create_tables(engine):
    Base.metadata.create_all(engine)
