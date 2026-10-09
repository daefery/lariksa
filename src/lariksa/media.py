"""Serve meeting audio to the UI so timestamps can jump to the exact moment.

A tiny HTTP server on 127.0.0.1 with a random port and a random per-run token
in every URL; only that token's holder (the app's own WebView) can fetch audio.
It supports HTTP Range, which <audio> needs to seek inside long recordings.

For recorded meetings the two tracks (mic + system) are mixed into one
playback file on first request and cached next to them; imported audio is
served as-is.
"""

from __future__ import annotations

import logging
import re
import secrets
import threading
import wave
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import numpy as np

log = logging.getLogger(__name__)

PLAYBACK = "playback.wav"

#: Fixed types: ``mimetypes`` reads the OS registry, which differs per machine
#: (Windows says audio/wav, Linux audio/x-wav) and can be edited by other apps.
CONTENT_TYPES = {
    ".wav": "audio/wav", ".mp3": "audio/mpeg", ".m4a": "audio/mp4", ".aac": "audio/aac",
    ".ogg": "audio/ogg", ".opus": "audio/ogg", ".flac": "audio/flac", ".webm": "audio/webm",
    ".mp4": "video/mp4", ".mkv": "video/x-matroska",
}
_ID = re.compile(r"^[\w-]{6,64}$")
_RANGE = re.compile(r"bytes=(\d*)-(\d*)$")


def mix_tracks(tracks: list[Path], target: Path) -> Path:
    """Average 16 kHz mono int16 WAVs into one file, sample-aligned from t=0."""
    data = []
    for t in tracks:
        with wave.open(str(t)) as w:
            data.append(np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).astype(np.int32))
            rate = w.getframerate()
    length = max(len(d) for d in data)
    mixed = np.zeros(length, dtype=np.int32)
    for d in data:
        mixed[: len(d)] += d
    # Sum, not average: two tracks rarely peak together, and averaging halves the
    # loudness of whoever is talking. Clip guards the rare overlap.
    mixed = np.clip(mixed, -32768, 32767).astype(np.int16)
    tmp = target.with_suffix(".tmp")
    with wave.open(str(tmp), "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(rate)
        out.writeframes(mixed.tobytes())
    tmp.replace(target)
    return target


class MediaServer:
    def __init__(self, folder_for):
        """``folder_for(meeting_id) -> Path`` gives the meeting's recording folder."""
        self._folder_for = folder_for
        self.token = secrets.token_urlsafe(18)
        self._lock = threading.Lock()
        server = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args):  # keep the app log clean
                pass

            def do_GET(self):  # noqa: N802 - http.server API
                server._handle(self)

        self._httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.port = self._httpd.server_address[1]
        threading.Thread(target=self._httpd.serve_forever, name="lariksa-media", daemon=True).start()

    def url(self, meeting_id: str) -> str:
        return f"http://127.0.0.1:{self.port}/{self.token}/{meeting_id}"

    def stop(self) -> None:
        self._httpd.shutdown()

    def audio_file(self, meeting_id: str) -> Path | None:
        folder = self._folder_for(meeting_id)
        if not folder.is_dir():
            return None
        tracks = [folder / n for n in ("mic.wav", "system.wav") if (folder / n).exists()
                  and (folder / n).stat().st_size > 1024]
        if tracks:
            cached = folder / PLAYBACK
            newest = max(t.stat().st_mtime for t in tracks)
            with self._lock:
                if not cached.exists() or cached.stat().st_mtime < newest:
                    mix_tracks(tracks, cached)
            return cached
        imported = sorted(p for p in folder.iterdir() if p.name.startswith("import."))
        return imported[0] if imported else None

    def _handle(self, req: BaseHTTPRequestHandler) -> None:
        parts = req.path.split("?")[0].strip("/").split("/")
        if len(parts) != 2 or not secrets.compare_digest(parts[0], self.token) or not _ID.match(parts[1]):
            req.send_error(404)
            return
        try:
            path = self.audio_file(parts[1])
        except Exception:  # pragma: no cover - corrupt WAV
            log.exception("Could not prepare audio for %s", parts[1])
            path = None
        if path is None:
            req.send_error(404)
            return

        size = path.stat().st_size
        start, end = 0, size - 1
        match = _RANGE.match(req.headers.get("Range", "").strip())
        if match:
            a, b = match.groups()
            if a:
                start, end = int(a), int(b) if b else size - 1
            elif b:  # suffix range: last N bytes
                start = max(size - int(b), 0)
            if start > end or start >= size:
                req.send_response(416)
                req.send_header("Content-Range", f"bytes */{size}")
                req.end_headers()
                return
            end = min(end, size - 1)

        req.send_response(206 if match else 200)
        req.send_header("Content-Type", CONTENT_TYPES.get(path.suffix.lower(), "application/octet-stream"))
        req.send_header("Accept-Ranges", "bytes")
        req.send_header("Content-Length", str(end - start + 1))
        if match:
            req.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        req.send_header("Cache-Control", "no-store")
        req.end_headers()
        with path.open("rb") as f:
            f.seek(start)
            remaining = end - start + 1
            try:
                while remaining > 0:
                    chunk = f.read(min(1 << 16, remaining))
                    if not chunk:
                        break
                    req.wfile.write(chunk)
                    remaining -= len(chunk)
            except (BrokenPipeError, ConnectionResetError):
                pass  # the player seeked elsewhere
