"""The meeting transcript: who said what, and when.

A meeting is recorded as two tracks. The microphone is you ("Saya"); the
system loopback is everyone else ("Peserta"). Each track is transcribed on its
own and the two are merged by time, which gives reliable speaker labels with
no diarization model.

The one trap is bleed: on laptop speakers the mic also hears the other
participants, so their words show up twice, once on each track. A mic utterance
that overlaps a system utterance in time and says nearly the same thing is
that echo, and is dropped.
"""

from __future__ import annotations

import re
from dataclasses import asdict, dataclass
from difflib import SequenceMatcher

ME = "Saya"
OTHERS = "Peserta"

#: Similarity above which an overlapping mic utterance is treated as echo.
BLEED_SIMILARITY = 0.6


@dataclass
class Utterance:
    speaker: str
    start: float
    end: float
    text: str

    def overlaps(self, other: Utterance) -> bool:
        return self.start < other.end and other.start < self.end

    def to_dict(self) -> dict:
        return asdict(self)


def _normalise(text: str) -> str:
    return re.sub(r"[^\w\s]", "", text.lower()).strip()


def _similar(a: str, b: str) -> float:
    return SequenceMatcher(None, _normalise(a), _normalise(b)).ratio()


def _is_echo(mine: Utterance, theirs: list[Utterance]) -> bool:
    for other in theirs:
        if other.start >= mine.end:
            break
        if mine.overlaps(other) and _similar(mine.text, other.text) >= BLEED_SIMILARITY:
            return True
    return False


def merge_tracks(mine: list[Utterance], theirs: list[Utterance]) -> list[Utterance]:
    """Interleave both tracks by start time, dropping mic echo of the others."""
    theirs = sorted(theirs, key=lambda u: u.start)
    kept = [u for u in mine if not _is_echo(u, theirs)]
    return sorted([*kept, *theirs], key=lambda u: (u.start, u.end))


def from_dicts(rows: list[dict]) -> list[Utterance]:
    return [Utterance(**row) for row in rows]


def clock(seconds: float) -> str:
    total = int(seconds)
    hours, rest = divmod(total, 3600)
    minutes, secs = divmod(rest, 60)
    return f"{hours}:{minutes:02d}:{secs:02d}" if hours else f"{minutes:02d}:{secs:02d}"


def as_text(utterances: list[Utterance]) -> str:
    """Plain text with speaker turns merged, the form the LLM reads."""
    lines: list[str] = []
    speaker = None
    for u in utterances:
        if u.speaker == speaker and lines:
            lines[-1] += " " + u.text
        else:
            lines.append(f"[{clock(u.start)}] {u.speaker}: {u.text}")
            speaker = u.speaker
    return "\n".join(lines)
