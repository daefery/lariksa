"""From audio files to a finished meeting: transcript, then notulen.

Both steps are pure orchestration over injected engines, so the flow is
testable without models and the app decides how engines are built.
"""

from __future__ import annotations

import logging
import time
from collections.abc import Callable
from contextlib import AbstractContextManager
from dataclasses import dataclass
from pathlib import Path

from .language import AUTO, resolve
from .notulen import write_notulen
from .recorder import MIC_FILE, SYSTEM_FILE
from .store import DONE, FAILED, QUEUED, SUMMARIZING, TRANSCRIBING, Store
from .transcript import ME, OTHERS, Utterance, as_text, merge_tracks

log = logging.getLogger(__name__)

#: Speaker label for an imported single-track file.
SPEAKER = "Pembicara"

Progress = Callable[[str, float], None]


class TranscriberLike(AbstractContextManager):
    def transcribe(self, audio: Path, speaker: str, on_progress=None, cancelled=None) -> list[Utterance]: ...


class LLMLike(AbstractContextManager):
    def complete_json(self, system: str, user: str, schema: dict) -> dict: ...


@dataclass
class Engines:
    transcriber: Callable[[str, tuple[str, ...]], TranscriberLike]
    """(language, glossary) -> transcriber context manager"""
    llm: Callable[[], LLMLike]


def tracks_for(folder: Path) -> list[tuple[Path, str]]:
    """Audio to transcribe and who is speaking on each."""
    mic, system = folder / MIC_FILE, folder / SYSTEM_FILE
    if mic.exists() or system.exists():
        return [(p, s) for p, s in ((mic, ME), (system, OTHERS)) if p.exists() and p.stat().st_size > 1024]
    imported = sorted(p for p in folder.iterdir() if p.is_file() and p.name.startswith("import."))
    return [(imported[0], SPEAKER)] if imported else []


def process(
    meeting_id: str,
    folder: Path,
    store: Store,
    engines: Engines,
    *,
    glossary: tuple[str, ...] = (),
    progress: Progress = lambda stage, value: None,
    cancelled: Callable[[], bool] = lambda: False,
    skip_transcription: bool = False,
    notulen_language: str = AUTO,
) -> None:
    """``notulen_language`` is "id", "en", or "auto" to follow what was spoken."""
    meeting = store.get(meeting_id)
    if meeting is None:
        return
    timings = dict(meeting.timings)
    try:
        utterances = meeting.transcript
        if not skip_transcription or not utterances:
            store.update(meeting_id, status=TRANSCRIBING, error="")
            started = time.monotonic()
            utterances = _transcribe(folder, meeting.language, glossary, engines, progress, cancelled)
            timings["transcribe_s"] = round(time.monotonic() - started, 1)
            store.update(meeting_id, transcript=utterances, timings=timings)

        if not utterances:
            store.update(meeting_id, status=FAILED,
                         error="Tidak ada suara yang terdeteksi. Cek mikrofon dan audio rapat.")
            return

        store.update(meeting_id, status=SUMMARIZING)
        progress("summarizing", 0.0)
        started = time.monotonic()
        spoken = " ".join(u.text for u in utterances)
        language = resolve(notulen_language, spoken, fallback=meeting.language or "id")
        with engines.llm() as llm:
            notulen = write_notulen(
                as_text(utterances), llm.complete_json, language=language,
                on_progress=lambda v: progress("summarizing", v),
            )
        timings["summarize_s"] = round(time.monotonic() - started, 1)

        fields = {"notulen": notulen, "status": DONE, "timings": timings, "error": ""}
        if notulen.title and meeting.has_default_title:  # never overwrite a user's title
            fields["title"] = notulen.title
        store.update(meeting_id, **fields)
    except Exception as exc:
        if cancelled():  # app is closing: pick the meeting up again next start
            store.update(meeting_id, status=QUEUED, timings=timings)
            return
        log.exception("Processing %s failed", meeting_id)
        store.update(meeting_id, status=FAILED, error=str(exc) or exc.__class__.__name__, timings=timings)


def _transcribe(folder: Path, language: str, glossary: tuple[str, ...], engines: Engines,
                progress: Progress, cancelled: Callable[[], bool]) -> list[Utterance]:
    tracks = tracks_for(folder)
    if not tracks:
        return []
    by_speaker: dict[str, list[Utterance]] = {}
    with engines.transcriber(language, glossary) as transcriber:
        for index, (path, speaker) in enumerate(tracks):
            def report(value: float, index=index) -> None:
                progress("transcribing", (index + value) / len(tracks))

            by_speaker[speaker] = transcriber.transcribe(path, speaker, report, cancelled)
    if SPEAKER in by_speaker:
        return by_speaker[SPEAKER]
    return merge_tracks(by_speaker.get(ME, []), by_speaker.get(OTHERS, []))
