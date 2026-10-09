"""Speech to text with faster-whisper, tuned for Indonesian meetings.

Three settings carry most of the accuracy (the engine is the same Whisper
everyone uses):

* the language is pinned instead of guessed from the first 30 seconds, which
  is how an English greeting turns forty minutes of Indonesian into nonsense;
* the glossary goes in twice: as ``hotwords`` to bias every window toward the
  team's names and jargon, and in the ``initial_prompt`` for spelling style;
* VAD drops silence, which both speeds things up and stops Whisper from
  inventing text over quiet stretches of the system track.
"""

from __future__ import annotations

import gc
import logging
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

from .hardware import Compute
from .transcript import Utterance

log = logging.getLogger(__name__)

#: Primes Whisper toward the register of a work meeting with English loanwords,
#: written the way Indonesian offices write them.
_BASE_PROMPT = {
    "id": "Rapat kerja tim. Kita bahas progress, deadline, budget, dan follow up minggu ini.",
    "en": "A team work meeting about progress, deadlines, budget and next steps.",
}


class TranscriptionError(RuntimeError):
    """Transcription could not be completed."""


@dataclass(frozen=True)
class TranscribeOptions:
    model_dir: Path
    compute: Compute
    #: "id", "en", or "" for auto-detect.
    language: str = "id"
    glossary: tuple[str, ...] = ()


def build_prompt(language: str, glossary: tuple[str, ...]) -> str:
    base = _BASE_PROMPT.get(language, _BASE_PROMPT["id"])
    if not glossary:
        return base
    return f"{base} Istilah: {', '.join(glossary)}."


class Transcriber:
    """Loads the model once, transcribes several tracks, then frees the memory.

    Use as a context manager: the LLM that writes the notulen runs right after,
    and an 8 GB laptop cannot hold both models at once.
    """

    def __init__(self, options: TranscribeOptions):
        self.options = options
        self._model = None
        self._pipeline = None

    def __enter__(self) -> Transcriber:
        try:
            from faster_whisper import BatchedInferencePipeline, WhisperModel
        except ImportError as exc:  # pragma: no cover
            raise TranscriptionError("faster-whisper tidak terpasang.") from exc
        c = self.options.compute
        try:
            self._model = WhisperModel(
                str(self.options.model_dir), device=c.device, compute_type=c.compute_type
            )
        except Exception as exc:
            raise TranscriptionError(f"Model transkripsi gagal dimuat: {exc}") from exc
        # Batched decoding is several times faster; keep batches small enough
        # for 4 GB GPUs and for CPUs that share memory with the meeting app.
        self._pipeline = BatchedInferencePipeline(self._model)
        return self

    def __exit__(self, *exc) -> None:
        self._pipeline = None
        self._model = None
        gc.collect()

    def transcribe(
        self,
        audio: Path,
        speaker: str,
        on_progress: Callable[[float], None] | None = None,
        cancelled: Callable[[], bool] = lambda: False,
    ) -> list[Utterance]:
        if self._pipeline is None:
            raise TranscriptionError("Transcriber used outside its context.")
        o = self.options
        language = o.language or None
        segments, info = self._pipeline.transcribe(
            str(audio),
            language=language,
            initial_prompt=build_prompt(o.language or "id", o.glossary),
            hotwords=", ".join(o.glossary) or None,
            batch_size=8 if o.compute.device == "cuda" else 4,
            vad_filter=True,
            vad_parameters={"min_silence_duration_ms": 500},
            # Repetition loops ("terima kasih terima kasih ...") are Whisper's
            # classic failure on long quiet stretches.
            repetition_penalty=1.05,
            no_repeat_ngram_size=0,
        )
        total = float(getattr(info, "duration", 0.0) or 0.0)
        utterances: list[Utterance] = []
        for seg in segments:
            if cancelled():
                raise TranscriptionError("Transkripsi dibatalkan.")
            text = (seg.text or "").strip()
            if text:
                utterances.append(Utterance(speaker, float(seg.start), float(seg.end), text))
            if on_progress and total:
                on_progress(min(1.0, float(seg.end) / total))
        if on_progress:
            on_progress(1.0)
        log.info("%s: %d utterances from %s", speaker, len(utterances), audio.name)
        return utterances
