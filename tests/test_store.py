from datetime import datetime

import pytest

from lariksa.notulen import Notulen
from lariksa.store import DONE, QUEUED, RECORDING, Store, normalise_glossary
from lariksa.transcript import ME, Utterance


@pytest.fixture
def store(tmp_path):
    return Store(tmp_path / "t.db")


def test_create_get_update_roundtrip(store):
    m = store.create(now=datetime(2026, 10, 9, 14, 30))
    assert m.title == "Rapat 9 Okt 2026, 14:30"  # Indonesian month, whatever the OS locale
    assert m.has_default_title
    assert m.status == RECORDING
    store.update(m.id, status=DONE, transcript=[Utterance(ME, 0, 1, "Halo")],
                 notulen=Notulen(title="Kickoff", summary="Mulai proyek"), timings={"asr": 1.5})
    got = store.get(m.id)
    assert got.status == DONE
    assert got.transcript[0].text == "Halo"
    assert got.notulen.title == "Kickoff"
    assert got.timings == {"asr": 1.5}


def test_search_finds_transcript_and_notulen_words(store):
    a = store.create(title="Sprint review")
    b = store.create(title="Budget")
    store.update(a.id, transcript=[Utterance(ME, 0, 1, "Deployment ke staging besok")])
    store.update(b.id, notulen=Notulen(summary="Anggaran pemasaran disetujui"))
    assert [m.id for m in store.list("staging")] == [a.id]
    assert [m.id for m in store.list("anggar")] == [b.id]  # prefix match
    assert len(store.list("")) == 2


def test_search_input_cannot_break_fts(store):
    store.create(title="x")
    assert store.list('"') == store.list("")
    assert store.list("AND OR (") == []


def test_unknown_field_rejected(store):
    m = store.create()
    with pytest.raises(ValueError):
        store.update(m.id, created_at="hack")


def test_pending_and_interrupted(store):
    a = store.create(status=QUEUED)
    b = store.create()
    assert store.pending() == [a.id]
    assert store.interrupted_recordings() == [b.id]


def test_settings_defaults_and_glossary_normalised(store):
    assert store.settings()["language"] == "id"
    saved = store.save_settings({"glossary": "Budi, PIC,\n budi , Solve Education", "evil": 1})
    assert saved["glossary"] == ["Budi", "PIC", "Solve Education"]
    assert "evil" not in saved


def test_normalise_glossary_caps_length():
    assert len(normalise_glossary([f"t{i}" for i in range(500)])) == 200
