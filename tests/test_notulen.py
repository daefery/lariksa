from lariksa.notulen import (
    SCHEMA,
    ActionItem,
    Notulen,
    chunks,
    to_markdown,
    to_whatsapp,
    write_notulen,
)

SAMPLE = {
    "title": "Sinkronisasi rilis",
    "summary": "Tim membahas rilis.",
    "topics": ["Status QA"],
    "decisions": ["Rilis hari Jumat"],
    "action_items": [{"task": "Cek bug login", "owner": "Budi", "due": "Kamis"}],
    "open_questions": [],
}


def test_short_transcript_is_one_call():
    calls = []

    def complete(system, user, schema):
        calls.append(user)
        assert schema is SCHEMA
        return SAMPLE

    n = write_notulen("[00:00] Saya: halo", complete)
    assert len(calls) == 1
    assert n.action_items == [ActionItem("Cek bug login", "Budi", "Kamis")]


def test_long_transcript_maps_then_reduces():
    calls = []

    def complete(system, user, schema):
        calls.append(user)
        return SAMPLE

    text = "\n".join(f"[00:{i:02d}] Peserta: " + "kata " * 400 for i in range(60))
    progress = []
    write_notulen(text, complete, on_progress=progress.append)
    expected_chunks = len(chunks(text))
    assert expected_chunks > 1
    assert len(calls) == expected_chunks + 1
    assert "Gabungkan" in calls[-1]
    assert progress[-1] == 1.0


def test_chunks_never_split_a_line():
    lines = [f"line {i} " + "x" * 50 for i in range(100)]
    parts = chunks("\n".join(lines), size=500)
    assert sum(p.count("\n") + 1 for p in parts) == 100
    assert all(line in "\n".join(parts) for line in lines)


def test_from_dict_tolerates_messy_model_output():
    n = Notulen.from_dict({
        "title": " Rapat ",
        "topics": ["", "  A  "],
        "action_items": [{"task": "X", "owner": None, "extra": 1}, {"task": ""}, "junk"],
    })
    assert n.title == "Rapat"
    assert n.topics == ["A"]
    assert n.action_items == [ActionItem("X", "", "")]


def test_markdown_and_whatsapp_render():
    n = Notulen.from_dict(SAMPLE)
    md = to_markdown(n, date="9 Okt 2026")
    assert "# Sinkronisasi rilis" in md
    assert "- [ ] Cek bug login (PIC: Budi, deadline: Kamis)" in md
    assert "## Pertanyaan terbuka\n- (tidak ada)" in md
    wa = to_whatsapp(n)
    assert wa.startswith("*Sinkronisasi rilis*")
    assert "• Cek bug login (PIC: Budi, deadline: Kamis)" in wa
    assert "Belum terjawab" not in wa
