import sys
import time
import wave

import pytest

from lariksa import service as service_mod
from lariksa.paths import Paths
from lariksa.pipeline import Engines
from lariksa.store import DONE, FAILED, RECORDING
from lariksa.transcript import ME, Utterance

from .test_pipeline import FakeLLM, FakeTranscriber


@pytest.fixture
def svc(tmp_path, monkeypatch):
    fake = FakeTranscriber({ME: [Utterance(ME, 0, 1, "Halo semua")],
                            "Pembicara": [Utterance("Pembicara", 0, 1, "Impor")]})
    monkeypatch.setattr(service_mod.LariksaService, "_engines",
                        lambda self, settings: Engines(lambda lang, g: fake, FakeLLM))
    return service_mod.LariksaService(Paths(tmp_path).ensure())


def wait_status(svc, meeting_id, statuses, timeout=5.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        m = svc.store.get(meeting_id)
        if m and m.status in statuses:
            return m
        time.sleep(0.02)
    raise AssertionError(f"status stuck at {svc.store.get(meeting_id).status}")


def test_import_runs_through_worker(svc, tmp_path):
    src = tmp_path / "rapat.m4a"
    src.write_bytes(b"x" * 4096)
    result = svc.import_audio(str(src))
    assert result["ok"]
    m = wait_status(svc, result["meeting_id"], {DONE, FAILED})
    assert m.status == DONE
    assert m.transcript[0].text == "Impor"
    detail = svc.meeting(m.id)
    assert "Deploy" in detail["markdown"] and detail["whatsapp"].startswith("*")


def test_worker_survives_an_engine_setup_error(tmp_path, monkeypatch):
    calls = {"n": 0}
    fake = FakeTranscriber({"Pembicara": [Utterance("Pembicara", 0, 1, "Halo")]})

    def engines(self, settings):
        calls["n"] += 1
        if calls["n"] == 1:
            raise RuntimeError("GPU driver hilang")
        return Engines(lambda lang, g: fake, FakeLLM)

    monkeypatch.setattr(service_mod.LariksaService, "_engines", engines)
    svc = service_mod.LariksaService(Paths(tmp_path).ensure())
    src = tmp_path / "a.wav"
    src.write_bytes(b"x" * 4096)
    first = svc.import_audio(str(src))["meeting_id"]
    assert wait_status(svc, first, {FAILED}).error == "GPU driver hilang"
    second = svc.import_audio(str(src))["meeting_id"]
    assert wait_status(svc, second, {DONE}).status == DONE  # worker still alive


def test_import_rejects_unknown_type(svc, tmp_path):
    bad = tmp_path / "notes.txt"
    bad.write_text("x")
    assert not svc.import_audio(str(bad))["ok"]


def test_crash_during_recording_is_recovered_on_start(tmp_path, monkeypatch):
    paths = Paths(tmp_path).ensure()
    first = service_mod.Store(paths.database)
    m = first.create(status=RECORDING)
    folder = paths.meeting_dir(m.id)
    folder.mkdir(parents=True)
    with wave.open(str(folder / "mic.wav"), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(16000)
        w.writeframes(b"\x01\x00" * 16000)

    fake = FakeTranscriber({ME: [Utterance(ME, 0, 1, "Selamat")]})
    monkeypatch.setattr(service_mod.LariksaService, "_engines",
                        lambda self, settings: Engines(lambda lang, g: fake, FakeLLM))
    svc = service_mod.LariksaService(paths)
    assert wait_status(svc, m.id, {DONE, FAILED}).status == DONE


def test_retry_summary_only_and_delete(svc, tmp_path):
    src = tmp_path / "a.wav"
    src.write_bytes(b"x" * 4096)
    mid = svc.import_audio(str(src))["meeting_id"]
    wait_status(svc, mid, {DONE})
    assert svc.retry(mid, summary_only=True)["ok"]
    assert wait_status(svc, mid, {DONE}).status == DONE
    assert svc.delete(mid)["ok"]
    assert svc.meeting(mid) is None
    assert not svc.paths.meeting_dir(mid).exists()


def test_failed_recording_start_leaves_nothing_behind(svc, monkeypatch):
    def broken(folder):
        folder.mkdir(parents=True)
        raise service_mod.RecorderError("Perangkat audio tidak ditemukan")

    monkeypatch.setattr(svc.recorder, "start", broken)
    result = svc.start_recording()
    assert result == {"ok": False, "error": "Perangkat audio tidak ditemukan"}
    assert svc.meetings() == []
    assert list(svc.paths.recordings.iterdir()) == []


@pytest.mark.skipif(sys.platform != "win32", reason="WASAPI is Windows-only")
def test_real_two_track_recording_on_windows(svc):
    started = svc.start_recording("Tes perekam")
    assert started["ok"], started
    time.sleep(2.0)
    stopped = svc.stop_recording()
    assert stopped["ok"]
    folder = svc.paths.meeting_dir(started["meeting_id"])
    for name in ("mic.wav", "system.wav"):
        with wave.open(str(folder / name)) as w:
            assert (w.getframerate(), w.getnchannels()) == (16000, 1)
            # Both tracks follow the wall clock even when nothing plays.
            assert 1.7 <= w.getnframes() / 16000 <= 2.5, (name, w.getnframes())


def test_status_reports_missing_assets(svc):
    s = svc.status()
    assert s["ready"] is False
    assert "whisper-turbo" in s["needed"] and "llm" in s["needed"]
    assert s["queue"] == 0 and s["recording"] is False


def test_rename_and_settings(svc, tmp_path):
    src = tmp_path / "a.wav"
    src.write_bytes(b"x" * 4096)
    mid = svc.import_audio(str(src))["meeting_id"]
    assert not svc.rename(mid, "  ")["ok"]
    assert svc.rename(mid, "Sprint 12")["ok"]
    assert svc.store.get(mid).title == "Sprint 12"
    assert svc.save_settings({"glossary": "Sari, Budi"})["glossary"] == ["Sari", "Budi"]
    assert svc.store.settings()["notulen_language"] == "auto"


def test_editing_an_english_notulen_keeps_it_english(svc, tmp_path):
    src = tmp_path / "a.wav"
    src.write_bytes(b"x" * 4096)
    mid = svc.import_audio(str(src))["meeting_id"]
    wait_status(svc, mid, {DONE})
    m = svc.store.get(mid)
    m.notulen.language = "en"
    svc.store.update(mid, notulen=m.notulen)
    # The UI sends edited fields only, without the language.
    svc.save_notulen(mid, {"title": "T", "summary": "Edited", "topics": [], "decisions": [],
                           "action_items": [], "open_questions": []})
    detail = svc.meeting(mid)
    assert detail["notulen"]["language"] == "en"
    assert detail["labels"]["summary"] == "Summary"
    assert "## Summary" in detail["markdown"]
