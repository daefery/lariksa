import pytest

from lariksa.notulen import SCHEMA
from lariksa.pipeline import SPEAKER, Engines, process, tracks_for
from lariksa.store import DONE, FAILED, Store
from lariksa.transcript import ME, OTHERS, Utterance


class FakeTranscriber:
    def __init__(self, script):
        self.script = script
        self.calls = []

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False

    def transcribe(self, audio, speaker, on_progress=None, cancelled=None):
        self.calls.append((audio.name, speaker))
        if on_progress:
            on_progress(1.0)
        return self.script.get(speaker, [])


class FakeLLM:
    def __init__(self, fail=False):
        self.fail = fail

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False

    def complete_json(self, system, user, schema):
        assert schema is SCHEMA
        if self.fail:
            raise RuntimeError("model crashed")
        return {"title": "Rapat rilis", "summary": "Ok", "topics": [], "decisions": [],
                "action_items": [{"task": "Deploy", "owner": "Budi", "due": "Jumat"}],
                "open_questions": []}


@pytest.fixture
def store(tmp_path):
    return Store(tmp_path / "t.db")


def two_tracks(folder):
    folder.mkdir(parents=True, exist_ok=True)
    (folder / "mic.wav").write_bytes(b"x" * 2048)
    (folder / "system.wav").write_bytes(b"x" * 2048)


def test_recorded_meeting_gets_merged_transcript_and_notulen(store, tmp_path):
    folder = tmp_path / "m"
    two_tracks(folder)
    m = store.create()
    fake = FakeTranscriber({
        ME: [Utterance(ME, 5, 6, "Siap, saya deploy")],
        OTHERS: [Utterance(OTHERS, 0, 4, "Tolong deploy Jumat ya")],
    })
    stages = []
    process(m.id, folder, store, Engines(lambda lang, g: fake, FakeLLM),
            progress=lambda s, v: stages.append(s))

    got = store.get(m.id)
    assert got.status == DONE
    assert [u.speaker for u in got.transcript] == [OTHERS, ME]
    assert got.title == "Rapat rilis"  # default title replaced
    assert got.notulen.action_items[0].owner == "Budi"
    assert {"transcribe_s", "summarize_s"} <= got.timings.keys()
    assert stages[0] == "transcribing" and stages[-1] == "summarizing"


def test_user_title_is_kept(store, tmp_path):
    folder = tmp_path / "m"
    two_tracks(folder)
    m = store.create(title="Rapat mingguan")  # looks like a default title, but isn't
    fake = FakeTranscriber({ME: [Utterance(ME, 0, 1, "Halo")]})
    process(m.id, folder, store, Engines(lambda lang, g: fake, FakeLLM))
    assert store.get(m.id).title == "Rapat mingguan"


def test_silent_meeting_fails_with_a_clear_message(store, tmp_path):
    folder = tmp_path / "m"
    two_tracks(folder)
    m = store.create()
    process(m.id, folder, store, Engines(lambda lang, g: FakeTranscriber({}), FakeLLM))
    got = store.get(m.id)
    assert got.status == FAILED
    assert "Tidak ada suara" in got.error


def test_llm_failure_keeps_transcript_and_retry_skips_asr(store, tmp_path):
    folder = tmp_path / "m"
    two_tracks(folder)
    m = store.create()
    fake = FakeTranscriber({ME: [Utterance(ME, 0, 1, "Halo")]})
    process(m.id, folder, store, Engines(lambda lang, g: fake, lambda: FakeLLM(fail=True)))
    got = store.get(m.id)
    assert got.status == FAILED and got.error == "model crashed"
    assert got.transcript

    fake.calls.clear()
    process(m.id, folder, store, Engines(lambda lang, g: fake, FakeLLM), skip_transcription=True)
    assert fake.calls == []
    assert store.get(m.id).status == DONE


def test_glossary_and_language_reach_the_transcriber(store, tmp_path):
    folder = tmp_path / "m"
    two_tracks(folder)
    m = store.create(language="en")
    seen = {}

    def make(language, glossary):
        seen.update(language=language, glossary=glossary)
        return FakeTranscriber({ME: [Utterance(ME, 0, 1, "Hi")]})

    process(m.id, folder, store, Engines(make, FakeLLM), glossary=("Lariksa",))
    assert seen == {"language": "en", "glossary": ("Lariksa",)}


def test_closing_the_app_mid_job_requeues_instead_of_failing(store, tmp_path):
    folder = tmp_path / "m"
    two_tracks(folder)
    m = store.create()

    class Interrupted(FakeTranscriber):
        def transcribe(self, *a, **k):
            raise RuntimeError("Transkripsi dibatalkan.")

    process(m.id, folder, store, Engines(lambda lang, g: Interrupted({}), FakeLLM),
            cancelled=lambda: True)
    assert store.get(m.id).status == "queued"
    assert store.pending() == [m.id]


def test_tracks_for_imported_file(tmp_path):
    (tmp_path / "import.m4a").write_bytes(b"x" * 10)
    assert tracks_for(tmp_path) == [(tmp_path / "import.m4a", SPEAKER)]


def test_tracks_for_skips_empty_track(tmp_path):
    (tmp_path / "mic.wav").write_bytes(b"x" * 2048)
    (tmp_path / "system.wav").write_bytes(b"x" * 44)
    assert tracks_for(tmp_path) == [(tmp_path / "mic.wav", ME)]
