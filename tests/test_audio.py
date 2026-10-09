import wave

import numpy as np

from lariksa.audio import TARGET_RATE, Downsampler, missing_frames, peak, repair_wav


def sine(rate, seconds, freq=440.0, channels=2):
    t = np.arange(int(rate * seconds)) / rate
    mono = (np.sin(2 * np.pi * freq * t) * 12000).astype(np.int16)
    return np.repeat(mono[:, None], channels, axis=1).ravel().tobytes()


def test_downsample_48k_stereo_to_16k_mono_keeps_duration():
    out = Downsampler(48_000, 2).process(sine(48_000, 1.0))
    assert abs(len(out) // 2 - TARGET_RATE) <= 2


def test_chunked_output_matches_one_shot():
    data = sine(44_100, 2.0)
    one = np.frombuffer(Downsampler(44_100, 2).process(data), dtype=np.int16)
    ds = Downsampler(44_100, 2)
    frame = 4  # bytes per stereo frame
    parts = [data[i:i + 1024 * frame] for i in range(0, len(data), 1024 * frame)]
    many = np.frombuffer(b"".join(ds.process(p) for p in parts), dtype=np.int16)
    n = min(len(one), len(many))
    assert abs(len(one) - len(many)) <= 1
    assert np.abs(one[:n].astype(int) - many[:n].astype(int)).max() <= 1


def test_tone_survives_downsampling():
    out = np.frombuffer(Downsampler(48_000, 1).process(sine(48_000, 1.0, channels=1)), dtype=np.int16)
    spectrum = np.abs(np.fft.rfft(out.astype(float)))
    assert abs(np.argmax(spectrum) * TARGET_RATE / len(out) - 440) < 3


def test_16k_mono_passes_through():
    data = sine(16_000, 0.5, channels=1)
    assert Downsampler(16_000, 1).process(data)[2:200] == data[2:200]


def test_peak():
    assert peak(b"") == 0.0
    assert peak(np.array([0, 16384, -32768], dtype=np.int16).tobytes()) == 1.0


def test_missing_frames_pads_only_real_gaps():
    assert missing_frames(10.0, 16_000, written=160_000, incoming=0, tolerance_s=0.25) == 0
    assert missing_frames(10.0, 16_000, written=150_000, incoming=0, tolerance_s=0.25) == 10_000
    assert missing_frames(10.0, 16_000, written=158_000, incoming=0, tolerance_s=0.25) == 0


def test_repair_wav_after_crash(tmp_path):
    path = tmp_path / "mic.wav"
    w = wave.open(str(path), "wb")
    w.setnchannels(1)
    w.setsampwidth(2)
    w.setframerate(16_000)
    w.writeframes(b"\x00\x00")  # header now says 1 frame
    w._file.flush()
    with path.open("ab") as f:  # simulate frames written after the header, then a crash
        f.write(b"\x01\x00" * 15_999)
    assert repair_wav(path)
    with wave.open(str(path)) as r:
        assert r.getnframes() == 16_000
    assert not repair_wav(path)  # already consistent
