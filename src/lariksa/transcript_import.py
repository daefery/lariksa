"""Import an existing transcript (Read.ai, Google Meet, Zoom, Teams, plain text).

Lariksa can then write its minutes for meetings it did not record. Supported
shapes, all common in exported transcripts:

    0:00 - Raka Aditya Pratama            (Read.ai, Meet notes)
    So first, a bit of context...

    [00:02:08] Nadia Kusuma: But also...   (one line per turn)

    00:02:08.000 --> 00:02:10.000          (WebVTT / Zoom / Teams)
    <v Nadia Kusuma>But also...</v>

Speaker turns end where the next one starts; the last turn gets a nominal
length so talk-time stats stay sane.
"""

from __future__ import annotations

import re
from pathlib import Path

from .transcript import Utterance

TEXT_TYPES = (".txt", ".vtt", ".srt", ".md")

_TIME = r"(\d{1,2}:\d{2}(?::\d{2})?(?:[.,]\d{1,3})?)"
_HEADER = re.compile(rf"^[ \t]*\[?{_TIME}\]?[ \t]*[-–—][ \t]*([^\n]+?)[ \t]*$", re.M)
_INLINE = re.compile(rf"^[ \t]*\[?{_TIME}\]?[ \t]+([^:\n]{{1,60}}?):[ \t]*(\S[^\n]*)$", re.M)
_VTT_CUE = re.compile(rf"{_TIME}\s*-->\s*{_TIME}[^\n]*\n(.+?)(?:\n\s*\n|\Z)", re.S)
_VTT_VOICE = re.compile(r"<v\s+([^>]+)>(.*?)(?:</v>|$)", re.S)

LAST_TURN_SECONDS = 10.0


def seconds(stamp: str) -> float:
    stamp = stamp.replace(",", ".")
    parts = [float(p) for p in stamp.split(":")]
    if len(parts) == 2:
        return parts[0] * 60 + parts[1]
    return parts[0] * 3600 + parts[1] * 60 + parts[2]


def _clean(body: str) -> str:
    return " ".join(body.split())


def _from_starts(turns: list[tuple[float, str, str]]) -> list[Utterance]:
    out = []
    for i, (start, speaker, text) in enumerate(turns):
        if not text:
            continue
        nxt = next((t[0] for t in turns[i + 1:] if t[0] > start), start + LAST_TURN_SECONDS)
        out.append(Utterance(speaker.strip() or "Pembicara", start, max(nxt, start + 1.0), text))
    return out


def parse(text: str) -> list[Utterance]:
    text = text.replace("\r\n", "\n").replace("\r", "\n").lstrip("\ufeff")

    if text.lstrip().upper().startswith("WEBVTT") or "-->" in text:
        utts = []
        for m in _VTT_CUE.finditer(text):
            start, end, body = seconds(m.group(1)), seconds(m.group(2)), m.group(3)
            voice = _VTT_VOICE.search(body)
            if voice:
                speaker, said = voice.group(1), voice.group(2)
            elif ":" in body.split("\n")[0][:60]:
                speaker, said = body.split(":", 1)
            else:
                speaker, said = "Pembicara", body
            said = _clean(re.sub(r"<[^>]+>", "", said))
            if said:
                utts.append(Utterance(speaker.strip(), start, max(end, start + 0.5), said))
        return utts

    headers = list(_HEADER.finditer(text))
    if headers:
        turns = []
        for i, m in enumerate(headers):
            body_end = headers[i + 1].start() if i + 1 < len(headers) else len(text)
            turns.append((seconds(m.group(1)), m.group(2), _clean(text[m.end():body_end])))
        return _from_starts(turns)

    inline = list(_INLINE.finditer(text))
    if inline:
        return _from_starts([(seconds(m.group(1)), m.group(2), _clean(m.group(3))) for m in inline])

    body = _clean(text)
    return [Utterance("Pembicara", 0.0, 60.0, body)] if body else []


def load(path: Path) -> list[Utterance]:
    return parse(path.read_text(encoding="utf-8", errors="replace"))
