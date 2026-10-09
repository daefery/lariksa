"""Small, pure audio helpers used by the recorder (no device access here).

Tracks are stored as 16 kHz mono 16-bit WAV, the format Whisper reads
natively: about 115 MB per hour per track instead of ~700 MB for 48 kHz
stereo, and no resampling step before transcription.
"""

from __future__ import annotations

import struct
from pathlib import Path

import numpy as np

TARGET_RATE = 16_000


class Downsampler:
    """Streaming int16 PCM -> 16 kHz mono int16, continuous across chunks.

    Mixes channels down, applies a short moving-average low-pass sized to the
    rate ratio (cheap anti-aliasing that is plenty for speech), then linearly
    interpolates at the target rate. The read position and the filter history
    carry over between chunks, so chunk boundaries leave no clicks or drift.
    """

    def __init__(self, rate: int, channels: int, target: int = TARGET_RATE):
        self.channels = max(1, channels)
        self.step = rate / target
        self.taps = max(1, round(self.step))
        self._history = np.zeros(self.taps, dtype=np.float32)
        # Positions index `signal`, where 0 is the previous chunk's last sample;
        # start at 1, the first new sample, so there is no added delay.
        self._pos = 1.0

    def process(self, chunk: bytes) -> bytes:
        samples = np.frombuffer(chunk, dtype=np.int16)
        usable = len(samples) - len(samples) % self.channels
        if usable == 0:
            return b""
        mono = samples[:usable].reshape(-1, self.channels).astype(np.float32).mean(axis=1)

        # history (taps samples) + new samples, smoothed; index 0 of `signal`
        # is the last sample of the previous chunk.
        joined = np.concatenate([self._history, mono])
        if self.taps > 1:
            kernel = np.ones(self.taps, dtype=np.float32) / self.taps
            smooth = np.convolve(joined, kernel, mode="valid")
        else:
            smooth = joined
        signal = smooth[-(len(mono) + 1):] if len(smooth) > len(mono) else np.concatenate([[0.0], smooth])

        positions = np.arange(self._pos, len(signal) - 1, self.step)
        out = np.interp(positions, np.arange(len(signal)), signal) if len(positions) else np.empty(0)
        self._pos = (positions[-1] + self.step if len(positions) else self._pos) - (len(signal) - 1)
        self._history = joined[-self.taps:]
        return np.clip(np.rint(out), -32768, 32767).astype(np.int16).tobytes()


def peak(chunk: bytes) -> float:
    """Peak level 0..1 of int16 PCM."""
    if len(chunk) < 2:
        return 0.0
    samples = np.frombuffer(chunk[: len(chunk) - len(chunk) % 2], dtype=np.int16)
    return min(1.0, float(np.abs(samples.astype(np.int32)).max()) / 32768)


def missing_frames(elapsed_s: float, rate: int, written: int, incoming: int, tolerance_s: float) -> int:
    """Frames of silence to insert so a track keeps up with the wall clock.

    WASAPI loopback delivers nothing while nothing plays; without padding,
    every timestamp after a quiet stretch would be early.
    """
    gap = int(elapsed_s * rate) - (written + incoming)
    return gap if gap > rate * tolerance_s else 0


def repair_wav(path: Path) -> bool:
    """Fix the size fields of a canonical 44-byte-header WAV left by a crash.

    Python's ``wave`` writes the final sizes only on close, so a recording
    interrupted by a crash or power loss has a header claiming no audio. The
    audio itself is intact; patching two integers makes it readable again.
    """
    size = path.stat().st_size
    if size <= 44:
        return False
    with path.open("r+b") as f:
        header = f.read(44)
        if header[:4] != b"RIFF" or header[8:12] != b"WAVE" or header[36:40] != b"data":
            return False
        riff, data = struct.unpack_from("<I", header, 4)[0], struct.unpack_from("<I", header, 40)[0]
        if riff == size - 8 and data == size - 44:
            return False
        f.seek(4)
        f.write(struct.pack("<I", size - 8))
        f.seek(40)
        f.write(struct.pack("<I", size - 44))
    return True
