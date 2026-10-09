"""Meetings, settings and the glossary in one SQLite file.

The transcript and notulen are stored as JSON on the meeting row; an FTS5
index over title, transcript and notulen powers search. One file next to the
app is the whole user library, which is what makes the folder portable.
"""

from __future__ import annotations

import json
import sqlite3
import threading
import uuid
from contextlib import contextmanager
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path

from .notulen import Notulen, to_markdown
from .transcript import Utterance, as_text, from_dicts

# Meeting lifecycle. "recording" is set while audio is captured; the worker
# moves a meeting through "queued" -> "transcribing" -> "summarizing" -> "done".
RECORDING, QUEUED, TRANSCRIBING, SUMMARIZING, DONE, FAILED = (
    "recording", "queued", "transcribing", "summarizing", "done", "failed",
)

_SCHEMA = """
CREATE TABLE IF NOT EXISTS meetings (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    duration    REAL NOT NULL DEFAULT 0,
    source      TEXT NOT NULL DEFAULT 'record',
    language    TEXT NOT NULL DEFAULT 'id',
    status      TEXT NOT NULL,
    error       TEXT NOT NULL DEFAULT '',
    transcript  TEXT NOT NULL DEFAULT '[]',
    notulen     TEXT NOT NULL DEFAULT '',
    timings     TEXT NOT NULL DEFAULT '{}'
);
CREATE VIRTUAL TABLE IF NOT EXISTS meetings_fts USING fts5(
    id UNINDEXED, title, body, tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
"""

DEFAULT_SETTINGS = {
    "language": "id",  # what is spoken: id | en | "" (auto-detect); drives transcription
    "notulen_language": "auto",  # what the notulen is written in: auto | id | en
    "whisper_model": "auto",  # auto | whisper-turbo | whisper-small
    "glossary": [],
    "theme": "light",
}


MONTHS_ID = ("Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des")


def human_date(created: datetime) -> str:
    """'9 Okt 2026, 14:30', independent of the OS locale."""
    return f"{created.day} {MONTHS_ID[created.month - 1]} {created.year}, {created:%H:%M}"


def default_title(created: datetime) -> str:
    """Title given to a meeting the user did not name; notulen may replace it."""
    return f"Rapat {human_date(created)}"


@dataclass
class Meeting:
    id: str
    title: str
    created_at: str
    status: str
    duration: float = 0.0
    source: str = "record"
    language: str = "id"
    error: str = ""
    transcript: list[Utterance] = field(default_factory=list)
    notulen: Notulen | None = None
    timings: dict = field(default_factory=dict)

    @property
    def has_default_title(self) -> bool:
        try:
            return self.title == default_title(datetime.fromisoformat(self.created_at))
        except ValueError:
            return False

    def summary_dict(self) -> dict:
        return {
            "id": self.id,
            "title": self.title,
            "created_at": self.created_at,
            "duration": self.duration,
            "status": self.status,
            "source": self.source,
            "preview": (self.notulen.summary if self.notulen else "")[:220],
            "action_count": len(self.notulen.action_items) if self.notulen else 0,
        }

    def to_dict(self) -> dict:
        return {
            **self.summary_dict(),
            "language": self.language,
            "error": self.error,
            "transcript": [u.to_dict() for u in self.transcript],
            "notulen": self.notulen.to_dict() if self.notulen else None,
            "timings": self.timings,
        }


def _row_to_meeting(row: sqlite3.Row) -> Meeting:
    notulen = json.loads(row["notulen"]) if row["notulen"] else None
    return Meeting(
        id=row["id"],
        title=row["title"],
        created_at=row["created_at"],
        status=row["status"],
        duration=row["duration"],
        source=row["source"],
        language=row["language"],
        error=row["error"],
        transcript=from_dicts(json.loads(row["transcript"])),
        notulen=Notulen.from_dict(notulen) if notulen else None,
        timings=json.loads(row["timings"] or "{}"),
    )


def _fts_query(text: str) -> str:
    """Each word becomes a quoted prefix term, so user input can't break FTS syntax."""
    terms = [t.replace('"', "") for t in text.split() if t.replace('"', "")]
    return " ".join(f'"{t}"*' for t in terms)


class Store:
    def __init__(self, path: Path):
        self.path = path
        self._lock = threading.Lock()
        self._conn = sqlite3.connect(path, check_same_thread=False)
        self._conn.row_factory = sqlite3.Row
        self._conn.execute("PRAGMA journal_mode=WAL")
        self._conn.executescript(_SCHEMA)

    @contextmanager
    def _tx(self):
        with self._lock, self._conn:
            yield self._conn

    # -- meetings --------------------------------------------------------

    def create(self, *, title: str = "", source: str = "record", language: str = "id",
               status: str = RECORDING, now: datetime | None = None) -> Meeting:
        now = now or datetime.now()
        meeting = Meeting(
            id=now.strftime("%Y%m%d-%H%M%S-") + uuid.uuid4().hex[:6],
            title=title or default_title(now),
            created_at=now.isoformat(timespec="seconds"),
            status=status,
            source=source,
            language=language,
        )
        with self._tx() as c:
            c.execute(
                "INSERT INTO meetings (id, title, created_at, source, language, status) "
                "VALUES (?, ?, ?, ?, ?, ?)",
                (meeting.id, meeting.title, meeting.created_at, source, language, status),
            )
        self._reindex(meeting.id)
        return meeting

    def get(self, meeting_id: str) -> Meeting | None:
        with self._lock:
            row = self._conn.execute("SELECT * FROM meetings WHERE id = ?", (meeting_id,)).fetchone()
        return _row_to_meeting(row) if row else None

    def list(self, query: str = "") -> list[Meeting]:
        with self._lock:
            if query.strip() and _fts_query(query):
                rows = self._conn.execute(
                    "SELECT m.* FROM meetings_fts f JOIN meetings m ON m.id = f.id "
                    "WHERE meetings_fts MATCH ? ORDER BY m.created_at DESC",
                    (_fts_query(query),),
                ).fetchall()
            else:
                rows = self._conn.execute("SELECT * FROM meetings ORDER BY created_at DESC").fetchall()
        return [_row_to_meeting(r) for r in rows]

    def update(self, meeting_id: str, **fields) -> None:
        allowed = {"title", "duration", "status", "error", "language", "transcript", "notulen", "timings"}
        unknown = set(fields) - allowed
        if unknown:
            raise ValueError(f"Unknown meeting fields: {unknown}")
        encoded = {}
        for key, value in fields.items():
            if key == "transcript":
                value = json.dumps([u.to_dict() for u in value], ensure_ascii=False)
            elif key == "notulen":
                value = json.dumps(value.to_dict(), ensure_ascii=False) if value else ""
            elif key == "timings":
                value = json.dumps(value)
            encoded[key] = value
        if not encoded:
            return
        assignments = ", ".join(f"{k} = ?" for k in encoded)
        with self._tx() as c:
            c.execute(f"UPDATE meetings SET {assignments} WHERE id = ?", (*encoded.values(), meeting_id))
        if {"title", "transcript", "notulen"} & set(fields):
            self._reindex(meeting_id)

    def delete(self, meeting_id: str) -> None:
        with self._tx() as c:
            c.execute("DELETE FROM meetings WHERE id = ?", (meeting_id,))
            c.execute("DELETE FROM meetings_fts WHERE id = ?", (meeting_id,))

    def _reindex(self, meeting_id: str) -> None:
        meeting = self.get(meeting_id)
        if meeting is None:
            return
        body = as_text(meeting.transcript)
        if meeting.notulen:
            body = to_markdown(meeting.notulen) + "\n" + body
        with self._tx() as c:
            c.execute("DELETE FROM meetings_fts WHERE id = ?", (meeting_id,))
            c.execute("INSERT INTO meetings_fts (id, title, body) VALUES (?, ?, ?)",
                      (meeting_id, meeting.title, body))

    def pending(self) -> list[str]:
        """Meetings to resume after a restart, oldest first."""
        with self._lock:
            rows = self._conn.execute(
                "SELECT id FROM meetings WHERE status IN (?, ?, ?) ORDER BY created_at",
                (QUEUED, TRANSCRIBING, SUMMARIZING),
            ).fetchall()
        return [r["id"] for r in rows]

    def interrupted_recordings(self) -> list[str]:
        with self._lock:
            rows = self._conn.execute("SELECT id FROM meetings WHERE status = ?", (RECORDING,)).fetchall()
        return [r["id"] for r in rows]

    # -- settings --------------------------------------------------------

    def settings(self) -> dict:
        with self._lock:
            rows = self._conn.execute("SELECT key, value FROM settings").fetchall()
        stored = {r["key"]: json.loads(r["value"]) for r in rows}
        return {**DEFAULT_SETTINGS, **{k: v for k, v in stored.items() if k in DEFAULT_SETTINGS}}

    def save_settings(self, values: dict) -> dict:
        clean = {k: v for k, v in values.items() if k in DEFAULT_SETTINGS}
        if "glossary" in clean:
            clean["glossary"] = normalise_glossary(clean["glossary"])
        with self._tx() as c:
            for key, value in clean.items():
                c.execute("INSERT INTO settings (key, value) VALUES (?, ?) "
                          "ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                          (key, json.dumps(value, ensure_ascii=False)))
        return self.settings()


def normalise_glossary(terms) -> list[str]:
    """Accept a list or comma/newline text; trim, dedupe case-insensitively, keep order."""
    if isinstance(terms, str):
        terms = terms.replace("\n", ",").split(",")
    seen: set[str] = set()
    out: list[str] = []
    for term in terms:
        term = str(term).strip()
        if term and term.lower() not in seen:
            seen.add(term.lower())
            out.append(term)
    return out[:200]
