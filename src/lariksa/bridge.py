"""The JavaScript API: thin wrappers over LariksaService plus native dialogs.

pywebview exposes every public method of this object as
``window.pywebview.api.<name>``. Business logic stays in the service.
"""

from __future__ import annotations

import re
import subprocess
import sys
from datetime import datetime
from pathlib import Path

from .service import IMPORT_TYPES, LariksaService


#: pywebview's file-type filters must match ``^[\w ]+\(\*.ext(;\*.ext)*\)$``,
#: so no punctuation in the description ("Audio/Video" raises ValueError).
IMPORT_FILTER = f"Audio dan video ({';'.join(f'*{ext}' for ext in IMPORT_TYPES)})"
EXPORT_FILTERS = {"docx": "Word (*.docx)", "md": "Markdown (*.md)"}


def _safe_filename(title: str) -> str:
    cleaned = re.sub(r'[<>:"/\\|?*\x00-\x1f]', "", title).strip().rstrip(".")
    return (cleaned or "Notulen")[:80]


class Bridge:
    def __init__(self, service: LariksaService):
        self._service = service
        self._window = None

    def attach(self, window) -> None:
        self._window = window

    # Each method is a one-liner on purpose: the UI contract is the service.
    def status(self) -> dict:
        return self._service.status()

    def meetings(self, query: str = "") -> list[dict]:
        return self._service.meetings(query)

    def meeting(self, meeting_id: str) -> dict | None:
        return self._service.meeting(meeting_id)

    def start_recording(self, title: str = "") -> dict:
        return self._service.start_recording(title)

    def stop_recording(self) -> dict:
        return self._service.stop_recording()

    def rename(self, meeting_id: str, title: str) -> dict:
        return self._service.rename(meeting_id, title)

    def save_notulen(self, meeting_id: str, data: dict) -> dict:
        return self._service.save_notulen(meeting_id, data)

    def retry(self, meeting_id: str, summary_only: bool = False) -> dict:
        return self._service.retry(meeting_id, summary_only)

    def delete(self, meeting_id: str) -> dict:
        return self._service.delete(meeting_id)

    def save_settings(self, values: dict) -> dict:
        return self._service.save_settings(values)

    def download(self, keys: list[str]) -> dict:
        return self._service.download(keys)

    def import_audio(self) -> dict:
        import webview

        picked = self._window.create_file_dialog(webview.FileDialog.OPEN, file_types=(IMPORT_FILTER,))
        if not picked:
            return {"ok": False, "error": ""}
        return self._service.import_audio(picked[0])

    def export(self, meeting_id: str, kind: str) -> dict:
        import webview

        m = self._service.store.get(meeting_id)
        if m is None:
            return {"ok": False, "error": "Rapat tidak ditemukan."}
        ext = "docx" if kind == "docx" else "md"
        name = f"{_safe_filename(m.title)} - {datetime.fromisoformat(m.created_at):%Y-%m-%d}.{ext}"
        target = self._window.create_file_dialog(
            webview.FileDialog.SAVE, directory=str(Path.home() / "Documents"), save_filename=name,
            file_types=(EXPORT_FILTERS[ext],))
        if not target:
            return {"ok": False, "error": ""}
        path = target if isinstance(target, str) else target[0]
        if ext == "docx":
            return self._service.export_docx(meeting_id, path)
        return self._service.export_markdown(meeting_id, path)

    def copy_text(self, text: str) -> dict:
        """Clipboard via Windows' clip.exe; it expects UTF-16 to keep non-ASCII intact."""
        if sys.platform != "win32":
            return {"ok": False, "error": "Clipboard hanya di Windows."}
        subprocess.run(["clip"], input=text.encode("utf-16-le"), check=False,
                       creationflags=getattr(subprocess, "CREATE_NO_WINDOW", 0))
        return {"ok": True}
