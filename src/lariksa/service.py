"""The application: one object the UI talks to.

Owns the store, the recorder, a single background worker (one heavy job at a
time, so a laptop never runs two models at once) and asset downloads. Every
public method returns plain JSON-able data, so the UI bridge stays thin.
"""

from __future__ import annotations

import logging
import queue
import shutil
import sys
import threading
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path

from . import catalog
from .audio import repair_wav
from .hardware import detect, register_cuda_libraries
from .llm import LocalLLM, server_binary, stop_all_servers
from .media import MediaServer
from .minutes import Action, Chapter, Minutes, Point, lines_from
from .minutes_render import labels, to_markdown, to_whatsapp
from .transcript_import import TEXT_TYPES, load as load_transcript
from .paths import Paths
from .pipeline import Engines, process
from .recorder import MIC_FILE, SYSTEM_FILE, Recorder, RecorderError
from .store import DONE, FAILED, QUEUED, RECORDING, Store, human_date
from .transcribe import Transcriber, TranscribeOptions

log = logging.getLogger(__name__)

AUDIO_TYPES = (".wav", ".mp3", ".m4a", ".mp4", ".webm", ".ogg", ".flac", ".aac", ".mkv")
#: Audio/video is transcribed; a transcript file (Read.ai, Meet, Zoom, VTT) skips straight to the notulen.
IMPORT_TYPES = AUDIO_TYPES + TEXT_TYPES


@dataclass
class Job:
    meeting_id: str
    retry_summary: bool = False


@dataclass
class Activity:
    """What the worker and downloader are doing right now, for the UI."""
    meeting_id: str = ""
    stage: str = ""
    progress: float = 0.0
    downloads: dict = field(default_factory=dict)


class LariksaService:
    def __init__(self, paths: Paths):
        self.paths = paths
        self.store = Store(paths.database)
        self.recorder = Recorder()
        self.activity = Activity()
        self._recording_id = ""
        self._jobs: queue.Queue[Job] = queue.Queue()
        self._cancel = threading.Event()
        self._download_lock = threading.Lock()
        self._record_lock = threading.Lock()
        self._cuda = register_cuda_libraries(catalog.CUDA_RUNTIME.path(paths))
        self.media = MediaServer(paths.meeting_dir)
        self._recover()
        threading.Thread(target=self._worker, name="lariksa-worker", daemon=True).start()

    # -- startup ---------------------------------------------------------

    def _recover(self) -> None:
        """Rescue meetings cut off by a crash or a closed laptop lid."""
        for meeting_id in self.store.interrupted_recordings():
            folder = self.paths.meeting_dir(meeting_id)
            for name in (MIC_FILE, SYSTEM_FILE):
                if (folder / name).exists():
                    repair_wav(folder / name)
            self.store.update(meeting_id, status=QUEUED)
        for meeting_id in self.store.pending():
            self._jobs.put(Job(meeting_id))

    # -- status ----------------------------------------------------------

    def status(self) -> dict:
        settings = self.store.settings()
        compute = detect(self._cuda)
        assets = {key: {"label": a.label, "size": a.size, "installed": a.installed(self.paths)}
                  for key, a in catalog.ASSETS.items()}
        whisper = self._whisper_asset(settings, compute.device)
        needed = [whisper.key, catalog.SUMMARY_MODEL.key, catalog.LLAMA_RUNTIME.key]
        if compute.gpu_available and sys.platform == "win32":
            needed.append(catalog.CUDA_RUNTIME.key)  # optional, offered when a GPU is present
        return {
            "recording": self.recorder.recording,
            "recording_id": self._recording_id,
            "elapsed": round(self.recorder.elapsed(), 1),
            "levels": self.recorder.levels(),
            "silent": self.recorder.silent_tracks() if self.recorder.recording else [],
            "devices": self.recorder.devices,
            "activity": {
                "meeting_id": self.activity.meeting_id,
                "stage": self.activity.stage,
                "progress": round(self.activity.progress, 3),
                "downloads": dict(self.activity.downloads),
            },
            "queue": self._jobs.qsize(),
            "compute": {"label": compute.label, "device": compute.device,
                        "gpu": compute.gpu_name, "cuda_ready": self._cuda},
            "assets": assets,
            "needed": needed,
            # CUDA is an accelerator, not a requirement: recording works without it.
            "ready": all(assets[k]["installed"] for k in needed if k != catalog.CUDA_RUNTIME.key),
            "settings": settings,
        }

    @staticmethod
    def _whisper_asset(settings: dict, device: str) -> catalog.Asset:
        choice = settings.get("whisper_model", "auto")
        if choice in (catalog.WHISPER_TURBO.key, catalog.WHISPER_SMALL.key):
            return catalog.ASSETS[choice]
        # Turbo everywhere by default: measured ~34x real time on a laptop GPU,
        # and still faster than real time on a modern CPU.
        return catalog.WHISPER_TURBO

    # -- downloads -------------------------------------------------------

    def download(self, keys: list[str]) -> dict:
        unknown = [k for k in keys if k not in catalog.ASSETS]
        if unknown:
            return {"ok": False, "error": f"Aset tidak dikenal: {unknown}"}
        if not self._download_lock.acquire(blocking=False):
            return {"ok": False, "error": "Unduhan lain sedang berjalan."}

        def run() -> None:
            try:
                for key in keys:
                    asset = catalog.ASSETS[key]
                    state = {"label": asset.label, "done": 0, "total": asset.size, "error": ""}
                    self.activity.downloads[key] = state
                    try:
                        catalog.install(asset, self.paths,
                                        progress=lambda d, t, s=state: s.update(done=d, total=t))
                        state["done"] = state["total"]
                        if key == catalog.CUDA_RUNTIME.key:
                            self._cuda = register_cuda_libraries(asset.path(self.paths))
                            detect.cache_clear()
                    except Exception as exc:
                        log.exception("Download %s failed", key)
                        state["error"] = str(exc)
                        return
                    finally:
                        if not state["error"]:
                            self.activity.downloads.pop(key, None)
            finally:
                self._download_lock.release()

        threading.Thread(target=run, name="lariksa-download", daemon=True).start()
        return {"ok": True}

    # -- recording -------------------------------------------------------

    def start_recording(self, title: str = "") -> dict:
        # The main window button, the pill and the global shortcut can all race here.
        with self._record_lock:
            if self.recorder.recording:
                return {"ok": False, "error": "Rekaman sudah berjalan."}
            return self._start_recording(title)

    def _start_recording(self, title: str) -> dict:
        settings = self.store.settings()
        meeting = self.store.create(title=title.strip(), language=settings["language"])
        try:
            devices = self.recorder.start(self.paths.meeting_dir(meeting.id))
        except RecorderError as exc:
            self.store.delete(meeting.id)
            shutil.rmtree(self.paths.meeting_dir(meeting.id), ignore_errors=True)
            return {"ok": False, "error": str(exc)}
        self._recording_id = meeting.id
        return {"ok": True, "meeting_id": meeting.id, "devices": devices}

    def stop_recording(self) -> dict:
        with self._record_lock:
            return self._stop_recording()

    def _stop_recording(self) -> dict:
        if not self.recorder.recording:
            return {"ok": False, "error": "Tidak ada rekaman yang berjalan."}
        duration = self.recorder.stop()
        meeting_id, self._recording_id = self._recording_id, ""
        self.store.update(meeting_id, duration=round(duration, 1), status=QUEUED)
        self._jobs.put(Job(meeting_id))
        return {"ok": True, "meeting_id": meeting_id}

    def import_audio(self, source: str, title: str = "") -> dict:
        path = Path(source)
        if not path.is_file() or path.suffix.lower() not in IMPORT_TYPES:
            return {"ok": False, "error": "Format file tidak didukung."}
        settings = self.store.settings()
        transcript = []
        if path.suffix.lower() in TEXT_TYPES:
            transcript = load_transcript(path)
            if not transcript:
                return {"ok": False, "error": "Transkrip kosong atau formatnya tidak dikenali."}
        meeting = self.store.create(title=title.strip() or path.stem, source="import",
                                    language=settings["language"], status=QUEUED)
        if transcript:
            self.store.update(meeting.id, transcript=transcript,
                              duration=round(max(u.end for u in transcript), 1))
        else:
            folder = self.paths.meeting_dir(meeting.id)
            folder.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, folder / f"import{path.suffix.lower()}")
        self._jobs.put(Job(meeting.id, retry_summary=bool(transcript)))
        return {"ok": True, "meeting_id": meeting.id}

    # -- meetings --------------------------------------------------------

    def meetings(self, query: str = "") -> list[dict]:
        return [m.summary_dict() for m in self.store.list(query)]

    def meeting(self, meeting_id: str) -> dict | None:
        m = self.store.get(meeting_id)
        if m is None:
            return None
        data = m.to_dict()
        # The numbered line view the notulen references point into.
        data["lines"] = [{"n": l.n, "start": l.start, "speaker": l.speaker, "text": l.text}
                         for l in lines_from(m.transcript)]
        data["has_audio"] = self._audio_tracks(meeting_id) != []
        data["audio_url"] = self.media.url(meeting_id) if data["has_audio"] else ""
        if m.notulen:
            data["labels"] = labels(m.notulen.language)
            data["markdown"] = to_markdown(m.notulen, date=_human_date(m.created_at), title=m.title)
            data["whatsapp"] = to_whatsapp(m.notulen, date=_human_date(m.created_at), title=m.title)
        return data

    def _audio_tracks(self, meeting_id: str) -> list[Path]:
        folder = self.paths.meeting_dir(meeting_id)
        if not folder.is_dir():
            return []
        return sorted(p for p in folder.iterdir() if p.suffix.lower() in AUDIO_TYPES and p.stat().st_size > 1024)

    def rename(self, meeting_id: str, title: str) -> dict:
        title = title.strip()
        if not title:
            return {"ok": False, "error": "Judul tidak boleh kosong."}
        self.store.update(meeting_id, title=title)
        return {"ok": True}

    def save_notulen(self, meeting_id: str, data: dict) -> dict:
        """Apply UI edits. Only text, owners, due dates and done flags change;
        anchors (refs, start times), speakers and the language always come from storage."""
        current = self.store.get(meeting_id)
        if current is None or current.notulen is None:
            return {"ok": False, "error": "Notulen belum tersedia."}
        merged = merge_edits(current.notulen, data)
        self.store.update(meeting_id, notulen=merged)
        return {"ok": True}

    def retry(self, meeting_id: str, summary_only: bool = False) -> dict:
        m = self.store.get(meeting_id)
        if m is None or m.status not in (DONE, FAILED):
            return {"ok": False, "error": "Rapat ini sedang diproses."}
        self.store.update(meeting_id, status=QUEUED, error="")
        self._jobs.put(Job(meeting_id, retry_summary=summary_only and bool(m.transcript)))
        return {"ok": True}

    def delete(self, meeting_id: str) -> dict:
        if meeting_id in (self._recording_id, self.activity.meeting_id):
            return {"ok": False, "error": "Rapat ini sedang direkam atau diproses."}
        self.store.delete(meeting_id)
        shutil.rmtree(self.paths.meeting_dir(meeting_id), ignore_errors=True)
        return {"ok": True}

    def save_settings(self, values: dict) -> dict:
        return self.store.save_settings(values)

    def export_docx(self, meeting_id: str, target: str) -> dict:
        from .export import write_docx

        m = self.store.get(meeting_id)
        if m is None or m.notulen is None:
            return {"ok": False, "error": "Notulen belum tersedia."}
        write_docx(m, Path(target), date=_human_date(m.created_at))
        return {"ok": True, "path": target}

    def export_markdown(self, meeting_id: str, target: str) -> dict:
        from .export import full_markdown

        m = self.store.get(meeting_id)
        if m is None:
            return {"ok": False, "error": "Rapat tidak ditemukan."}
        Path(target).write_text(full_markdown(m, date=_human_date(m.created_at)), encoding="utf-8")
        return {"ok": True, "path": target}

    # -- worker ----------------------------------------------------------

    def _engines(self, settings: dict) -> Engines:
        compute = detect(self._cuda)
        whisper = self._whisper_asset(settings, compute.device)

        def transcriber(language: str, glossary: tuple[str, ...]) -> Transcriber:
            if not whisper.installed(self.paths):
                raise RuntimeError("Model transkripsi belum diunduh. Buka Pengaturan.")
            return Transcriber(TranscribeOptions(whisper.path(self.paths), compute, language, glossary))

        def llm() -> LocalLLM:
            model = catalog.SUMMARY_MODEL.path(self.paths) / catalog.SUMMARY_MODEL.marker
            return LocalLLM(server_binary(catalog.LLAMA_RUNTIME.path(self.paths)), model)

        return Engines(transcriber=transcriber, llm=llm)

    def _worker(self) -> None:
        while True:
            job = self._jobs.get()
            m = self.store.get(job.meeting_id)
            if m is None or m.status == RECORDING:
                continue
            settings = self.store.settings()
            self.activity.meeting_id, self.activity.stage, self.activity.progress = m.id, "queued", 0.0

            def progress(stage: str, value: float) -> None:
                self.activity.stage, self.activity.progress = stage, value

            try:
                process(m.id, self.paths.meeting_dir(m.id), self.store, self._engines(settings),
                        glossary=tuple(settings["glossary"]), progress=progress,
                        cancelled=self._cancel.is_set, skip_transcription=job.retry_summary,
                        notulen_language=settings["notulen_language"])
            except Exception as exc:  # never let one meeting kill the worker for the session
                log.exception("Worker failed on %s", m.id)
                self.store.update(m.id, status=FAILED, error=str(exc) or exc.__class__.__name__)
            finally:
                self.activity.meeting_id, self.activity.stage, self.activity.progress = "", "", 0.0

    def shutdown(self) -> None:
        """Save any recording; interrupted processing resumes on next start."""
        if self.recorder.recording:
            self.stop_recording()
        self._cancel.set()
        stop_all_servers()
        self.media.stop()


def _human_date(iso: str) -> str:
    try:
        return human_date(datetime.fromisoformat(iso))
    except ValueError:
        return iso


def merge_edits(stored: Minutes, data: dict) -> Minutes:
    """Overlay editable fields from the UI onto the stored minutes.

    Lists are matched by position; the UI may delete items (shorter list) but
    cannot invent anchors, and extra items it sends become unanchored notes.
    """
    def text(value, fallback=""):
        return " ".join(str(value if value is not None else fallback).split())

    def overlay(items, edits, make, apply):
        out = []
        for i, edit in enumerate(edits or []):
            if not isinstance(edit, dict):
                continue
            base = items[i] if i < len(items) else make()
            if apply(base, edit):
                out.append(base)
        return out

    def apply_action(a, e):
        a.task, a.owner, a.due = text(e.get("task"), a.task), text(e.get("owner"), a.owner), text(e.get("due"), a.due)
        a.done = bool(e.get("done", a.done))
        return bool(a.task)

    def apply_point(p, e):
        p.text = text(e.get("text"), p.text)
        return bool(p.text)

    def apply_chapter(c, e):
        c.title, c.summary = text(e.get("title"), c.title), text(e.get("summary"), c.summary)
        return bool(c.title or c.summary)

    out = Minutes.from_dict(stored.to_dict())
    out.title = text(data.get("title"), out.title)
    out.overview = text(data.get("overview"), out.overview)
    if "highlights" in data:
        out.highlights = [text(h) for h in data.get("highlights") or [] if text(h)]
    if "actions" in data:
        out.actions = overlay(out.actions, data["actions"], lambda: Action(""), apply_action)
    if "decisions" in data:
        out.decisions = overlay(out.decisions, data["decisions"], lambda: Point(""), apply_point)
    if "questions" in data:
        out.questions = overlay(out.questions, data["questions"], lambda: Point(""), apply_point)
    if "chapters" in data:
        out.chapters = overlay(out.chapters, data["chapters"], lambda: Chapter("", ""), apply_chapter)
    return out
