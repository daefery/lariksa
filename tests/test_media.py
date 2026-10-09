import wave

import httpx
import numpy as np
import pytest

from lariksa.media import PLAYBACK, MediaServer, mix_tracks


def wav(path, samples):
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(16000)
        w.writeframes(np.asarray(samples, dtype=np.int16).tobytes())


@pytest.fixture
def server(tmp_path):
    srv = MediaServer(lambda mid: tmp_path / mid)
    yield srv
    srv.stop()


def test_mix_aligns_and_clips(tmp_path):
    wav(tmp_path / "a.wav", [1000, 30000, 0])
    wav(tmp_path / "b.wav", [500, 10000])
    out = mix_tracks([tmp_path / "a.wav", tmp_path / "b.wav"], tmp_path / "m.wav")
    with wave.open(str(out)) as w:
        assert np.frombuffer(w.readframes(9), dtype=np.int16).tolist() == [1500, 32767, 0]


def test_serves_mixed_tracks_with_ranges(server, tmp_path):
    folder = tmp_path / "20261009-083051-bfb80e"
    folder.mkdir()
    wav(folder / "mic.wav", np.zeros(16000))
    wav(folder / "system.wav", np.ones(16000))
    url = server.url(folder.name)

    full = httpx.get(url)
    assert full.status_code == 200 and full.headers["content-type"] == "audio/wav"
    assert (folder / PLAYBACK).exists()

    part = httpx.get(url, headers={"Range": "bytes=44-143"})
    assert part.status_code == 206
    assert part.headers["content-range"] == f"bytes 44-143/{len(full.content)}"
    assert part.content == full.content[44:144]

    tail = httpx.get(url, headers={"Range": "bytes=-10"})
    assert tail.content == full.content[-10:]
    assert httpx.get(url, headers={"Range": f"bytes={len(full.content) + 5}-"}).status_code == 416


def test_wrong_token_or_path_is_404(server, tmp_path):
    (tmp_path / "abcdef123").mkdir()
    wav(tmp_path / "abcdef123" / "mic.wav", np.zeros(4000))
    base = f"http://127.0.0.1:{server.port}"
    assert httpx.get(f"{base}/wrong-token/abcdef123").status_code == 404
    assert httpx.get(f"{base}/{server.token}/..%2F..%2Fetc").status_code == 404
    assert httpx.get(f"{base}/{server.token}/missing-meeting").status_code == 404
    assert httpx.get(server.url("abcdef123")).status_code == 200


def test_imported_audio_served_as_is(server, tmp_path):
    folder = tmp_path / "import-meeting"
    folder.mkdir()
    (folder / "import.mp3").write_bytes(b"ID3" + b"\x00" * 2000)
    r = httpx.get(server.url("import-meeting"))
    assert r.status_code == 200 and r.headers["content-type"] == "audio/mpeg" and r.content.startswith(b"ID3")
