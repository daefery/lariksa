"""Two-track meeting recorder for Windows (WASAPI via PyAudioWPatch).

``mic.wav`` is the default microphone, ``system.wav`` is a loopback of the
default speakers/headset, i.e. everyone else in Meet, Zoom or Teams. Keeping
them separate is what gives "Saya" vs "Peserta" labels for free.

WASAPI loopback delivers nothing while nothing plays, so a naive recording of
a quiet stretch is shorter and every later timestamp drifts. Two guards keep
both tracks on the wall clock: a silent output stream keeps the audio engine
ticking, and the writer pads silence for any gap that still slips through.
"""

from __future__ import annotations

import logging
import queue
import threading
import time
import wave
from dataclasses import dataclass, field
from pathlib import Path

from .audio import TARGET_RATE, Downsampler, missing_frames, peak

log = logging.getLogger(__name__)

MIC_FILE = "mic.wav"
SYSTEM_FILE = "system.wav"
_FRAMES = 1024
#: Gaps longer than this (seconds) in a track are filled with silence.
_GAP_TOLERANCE = 0.25


class RecorderError(RuntimeError):
    """Recording could not start or failed."""


@dataclass
class _Track:
    path: Path
    resample: Downsampler
    queue: queue.Queue = field(default_factory=queue.Queue)
    written: int = 0
    level: float = 0.0
    loudest: float = 0.0


class Recorder:
    def __init__(self) -> None:
        self._pa = None
        self._streams: list = []
        self._threads: list[threading.Thread] = []
        self._tracks: dict[str, _Track] = {}
        self._running = threading.Event()
        self._started = 0.0
        self.devices: dict[str, str] = {}

    @property
    def recording(self) -> bool:
        return self._running.is_set()

    def elapsed(self) -> float:
        return time.monotonic() - self._started if self.recording else 0.0

    def levels(self) -> dict[str, float]:
        return {name: round(t.level, 3) for name, t in self._tracks.items()}

    def silent_tracks(self, after_s: float = 8.0) -> list[str]:
        """Tracks with no sound at all so far, typically a muted mic.

        Digital near-zero for several seconds is not a quiet room (a real mic
        always has a noise floor); it is a mute key or a disabled device.
        """
        if self.elapsed() < after_s:
            return []
        return [name for name, t in self._tracks.items() if t.loudest < 0.002]

    def start(self, folder: Path) -> dict[str, str]:
        if self.recording:
            raise RecorderError("Rekaman sudah berjalan.")
        try:
            import pyaudiowpatch as pyaudio
        except ImportError as exc:
            raise RecorderError("Perekam audio hanya tersedia di Windows.") from exc

        folder.mkdir(parents=True, exist_ok=True)
        pa = pyaudio.PyAudio()
        self._pa = pa
        try:
            wasapi = pa.get_host_api_info_by_type(pyaudio.paWASAPI)
            mic = pa.get_device_info_by_index(wasapi["defaultInputDevice"])
            speakers = pa.get_device_info_by_index(wasapi["defaultOutputDevice"])
            loopback = self._loopback_for(pa, speakers)
        except (OSError, LookupError) as exc:
            pa.terminate()
            self._pa = None
            raise RecorderError(f"Perangkat audio tidak ditemukan: {exc}") from exc

        self.devices = {"mic": mic["name"], "system": speakers["name"]}
        specs = {
            "mic": (folder / MIC_FILE, mic, max(1, min(2, int(mic["maxInputChannels"])))),
            "system": (folder / SYSTEM_FILE, loopback, max(1, int(loopback["maxInputChannels"]))),
        }
        self._tracks = {
            name: _Track(path, Downsampler(int(dev["defaultSampleRate"]), channels))
            for name, (path, dev, channels) in specs.items()
        }
        self._started = time.monotonic()
        self._running.set()
        try:
            self._keep_engine_awake(pa, speakers)
            for name, (_, dev, channels) in specs.items():
                self._open(pa, name, dev, channels)
        except Exception as exc:
            self.stop()
            raise RecorderError(f"Gagal mulai merekam: {exc}") from exc
        log.info("Recording mic=%s system=%s", mic["name"], speakers["name"])
        return self.devices

    @staticmethod
    def _loopback_for(pa, speakers: dict) -> dict:
        if speakers.get("isLoopbackDevice"):
            return speakers
        for device in pa.get_loopback_device_info_generator():
            if speakers["name"] in device["name"]:
                return device
        raise LookupError(f"loopback untuk '{speakers['name']}'")

    def _keep_engine_awake(self, pa, speakers: dict) -> None:
        import pyaudiowpatch as pyaudio

        channels = max(1, int(speakers["maxOutputChannels"]))

        def callback(_in, frames, _time, _status):
            return (bytes(frames * channels * 2), pyaudio.paContinue)

        try:
            stream = pa.open(format=pyaudio.paInt16, channels=channels,
                             rate=int(speakers["defaultSampleRate"]), output=True,
                             output_device_index=speakers["index"], frames_per_buffer=_FRAMES,
                             stream_callback=callback)
            self._streams.append(stream)
        except Exception as exc:  # padding in the writer still covers gaps
            log.warning("Silent keep-alive stream unavailable: %s", exc)

    def _open(self, pa, name: str, device: dict, channels: int) -> None:
        import pyaudiowpatch as pyaudio

        track = self._tracks[name]

        def callback(data, _frames, _time, _status):
            if self._running.is_set():
                track.queue.put(data)
            return (None, pyaudio.paContinue)

        stream = pa.open(format=pyaudio.paInt16, channels=channels,
                         rate=int(device["defaultSampleRate"]), input=True,
                         input_device_index=device["index"], frames_per_buffer=_FRAMES,
                         stream_callback=callback)
        writer = threading.Thread(target=self._write, args=(track,), name=f"rec-{name}", daemon=True)
        writer.start()
        self._streams.append(stream)
        self._threads.append(writer)

    def _write(self, track: _Track) -> None:
        with wave.open(str(track.path), "wb") as out:
            out.setnchannels(1)
            out.setsampwidth(2)
            out.setframerate(TARGET_RATE)
            while True:
                try:
                    chunk = track.queue.get(timeout=_GAP_TOLERANCE)
                except queue.Empty:
                    chunk = b""
                if chunk is None:
                    break
                pcm = track.resample.process(chunk) if chunk else b""
                self._pad(track, out, incoming=len(pcm) // 2)
                if pcm:
                    out.writeframes(pcm)
                    track.written += len(pcm) // 2
                    level = peak(pcm)
                    track.level = max(level, track.level * 0.85)
                    track.loudest = max(track.loudest, level)
                else:
                    track.level *= 0.85
            self._pad(track, out, incoming=0)

    def _pad(self, track: _Track, out: wave.Wave_write, incoming: int) -> None:
        gap = missing_frames(time.monotonic() - self._started, TARGET_RATE,
                             track.written, incoming, _GAP_TOLERANCE)
        if gap:
            out.writeframes(bytes(gap * 2))
            track.written += gap

    def stop(self) -> float:
        """Stop and flush both files. Returns the recorded duration in seconds."""
        duration = self.elapsed()
        self._running.clear()
        for stream in self._streams:
            try:
                stream.stop_stream()
                stream.close()
            except Exception:  # pragma: no cover - device already gone
                pass
        for track in self._tracks.values():
            track.queue.put(None)
        for thread in self._threads:
            thread.join(timeout=10)
        if self._pa is not None:
            self._pa.terminate()
        self._pa, self._streams, self._threads = None, [], []
        for track in self._tracks.values():
            track.level = 0.0
        return duration
