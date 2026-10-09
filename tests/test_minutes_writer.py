from lariksa.minutes import lines_from
from lariksa.minutes import Line
from lariksa.minutes_writer import (
    ACTIONS_SCHEMA,
    OVERVIEW_SCHEMA,
    REVIEW_SCHEMA,
    TOPICS_SCHEMA,
    merge_close_chapters,
    _ground,
    chunk_lines,
    clean_text,
    ground_due,
    write_minutes,
)
from lariksa.transcript import Utterance


def meeting(n=60):
    names = ["Nadia Kusuma", "Raka Aditya Pratama", "Bima Satria Wijaya"]
    return lines_from([Utterance(names[i % 3], i * 30, i * 30 + 25,
                                 f"Kalimat ke {i} tentang rilis tanggal 16 Okt dan migrasi data. " * 4)
                       for i in range(n)])


class FakeModel:
    """Answers like a small model: messy refs, a placeholder due date, one invented line."""

    def __init__(self):
        self.calls = []

    def __call__(self, system, user, schema):
        self.calls.append((system, user, schema))
        if schema is REVIEW_SCHEMA:
            # Keep every candidate, sharpen the wording of the first, and try an invalid id.
            n = user.count("owner=")
            return {"items": [{"id": 1, "verdict": "follow_up", "owner": "Raka", "task": "Deploy Edu.Platform ke production"},
                              *[{"id": i, "verdict": "follow_up", "owner": "", "task": ""} for i in range(2, n + 1)],
                              {"id": 999, "verdict": "follow_up", "owner": "", "task": "x"}]}
        if schema is OVERVIEW_SCHEMA:
            return {"title": "Sinkronisasi rilis", "overview": "Rapat membahas rilis.",
                    "highlights": ["Rilis 16 Oktober", "", "Migrasi 91 persen"]}
        first = int(user.split("\nL")[1].split(" ")[0]) if "\nL" in user else 1
        if schema is ACTIONS_SCHEMA:
            return {"actions": [
                {"quote": "aku deploy", "kind": "will_do", "owner": "Raka", "task": "Deploy ke production tanggal 16",
                 "due": "nanti", "refs": [f"L{first + 1}"]},
                {"quote": "deploy tgl 16", "kind": "will_do", "owner": "Raka",
                 "task": "Deploy ke production pada tanggal 16", "due": "16 Okt", "refs": [f"L{first + 2}"]},
                {"quote": "sudah fix", "kind": "already_done", "owner": "Raka", "task": "Fix edge case",
                 "due": "", "refs": [f"L{first}"]}]}
        return {
            "chapters": [{"title": "Rencana rilis", "summary": "Raka menjelaskan rilis.",
                          "refs": [f"L{first}", f"L{first + 1}"]},
                         {"title": "Topik tanpa bukti", "summary": "x", "refs": ["L99999"]}],
            "decisions": [{"text": "Prototype dikunci", "refs": [f"L{first}"]}],
            "questions": [{"text": "Siapa yang testing?", "refs": []}],
        }


def test_chunks_cover_every_line_once():
    lines = meeting()
    chunks = chunk_lines(lines, size=3000)
    assert len(chunks) > 2
    assert [l.n for c in chunks for l in c] == [l.n for l in lines]


def test_write_minutes_grounds_merges_and_anchors():
    lines = meeting()
    model = FakeModel()
    progress = []
    m = write_minutes(lines, model, language="id", on_progress=progress.append)

    chunks = len(chunk_lines(lines))
    assert len(model.calls) == 2 * chunks + 2  # topics + actions per chunk, review, overview
    assert [c[2] for c in model.calls[:-2]] == [TOPICS_SCHEMA, ACTIONS_SCHEMA] * chunks
    assert model.calls[-2][2] is REVIEW_SCHEMA and "kutipan=" in model.calls[-2][1]
    assert m.actions[0].task == "Deploy Edu.Platform ke production"  # review sharpened the wording
    assert progress[-1] == 1.0

    assert m.title == "Sinkronisasi rilis"
    assert m.highlights == ["Rilis 16 Oktober", "Migrasi 91 persen"]
    # The chapter citing a non-existent line is dropped; the real ones are anchored to their first line.
    assert all(c.title != "Topik tanpa bukti" for c in m.chapters)
    assert m.chapters[0].start == lines[0].start
    assert [c.start for c in m.chapters] == sorted(c.start for c in m.chapters)
    # Owner resolved to the speaker's full name; spoken due kept, placeholder cleared; twins merged.
    assert all(a.owner == "Raka Aditya Pratama" for a in m.actions)
    assert any(a.due == "16 Okt" for a in m.actions)
    assert not any(a.due == "nanti" for a in m.actions)
    assert not any(a.task == "Fix edge case" for a in m.actions)  # already done, not a follow-up
    # Questions without refs never reach the page; decisions dedupe across chunks.
    assert m.questions == []
    assert len(m.decisions) == 1 and len(m.decisions[0].refs) == chunks
    assert m.speakers[0].name in {"Nadia Kusuma", "Raka Aditya Pratama", "Bima Satria Wijaya"}


def test_english_prompts_for_english_minutes():
    model = FakeModel()
    write_minutes(meeting(6), model, language="en")
    system, user, _ = model.calls[0]
    assert "careful meeting note-taker" in system and "Part 1 of" in user


# Real failures seen from Qwen3-4B on an 80-minute Indonesian meeting.

def test_clean_text_strips_leaked_refs_and_clock_times():
    assert clean_text("Nama Play Class ditolak board [L6, L7, L8].") == "Nama Play Class ditolak board."
    assert clean_text("Nadia menyampaikan pada hari 04:23 bahwa platform dipisah") == (
        "Nadia menyampaikan bahwa platform dipisah")
    assert clean_text("Migrasi 91 persen, pipeline 84 persen") == "Migrasi 91 persen, pipeline 84 persen"


def test_due_dates_must_be_spoken():
    said = "Terus setelah itu di tanggal 16, terus di deploy produktinya di tanggal 16"
    assert ground_due("2024-10-16", said) == "tanggal 16"          # invented year -> spoken phrase
    assert ground_due("04:45-05:43 (dalam rapat)", "aku lagi ngajek ke tas") == ""  # clock time is not a due
    assert ground_due("minggu depan", "kita udah mulai minggu depan") == "minggu depan"
    assert ground_due("", "tanggal 30") == ""                      # no due claimed -> none added


def test_in_meeting_statements_are_not_action_items():
    lines = [Line(1, 0, 10, "Yoga", "Jadi scoring automation itu masih hardcoded."),
             Line(2, 10, 20, "Raka", "Aku deploy ke production tanggal 16 ya.")]
    raw = {"chapters": [], "decisions": [], "questions": [], "actions": [
        {"kind": "will_do", "owner": "Yoga", "task": "Menjelaskan proses scoring otomatis", "due": "", "refs": ["L1"]},
        {"kind": "discussion", "owner": "Raka", "task": "Deploy ke production", "due": "", "refs": ["L2"]},
        {"kind": "will_do", "owner": "Raka", "task": "Deploy ke production", "due": "2025-10-16", "refs": ["L2"]},
    ]}
    _, actions, _, _ = _ground(raw, lines, lines, ["Yoga", "Raka"])
    assert [(a.owner, a.task, a.due) for a in actions] == [("Raka", "Deploy ke production", "tanggal 16")]


def test_invented_questions_are_dropped():
    lines = [Line(1, 0, 10, "Nadia", "Backend nyatu, sip."),
             Line(2, 10, 20, "Velia", "Tanggal 30 bisa dipakai buat games nggak?")]
    raw = {"chapters": [], "actions": [], "decisions": [], "questions": [
        {"text": "Apakah integrasi backend memengaruhi keamanan?", "refs": ["L1"]},
        {"text": "Bisakah platform dipakai untuk games tanggal 30?", "refs": ["L2"]},
    ]}
    _, _, _, questions = _ground(raw, lines, lines, [])
    assert [q.text for q in questions] == ["Bisakah platform dipakai untuk games tanggal 30?"]


def test_empty_meeting_skips_overview_call():
    model = FakeModel()
    m = write_minutes([], model)
    assert model.calls == [] and m.chapters == [] and m.title == ""


def test_chapters_starting_close_together_merge():
    from lariksa.minutes import Chapter
    out = merge_close_chapters([Chapter("A", "short", 156, [6]), Chapter("B", "a longer summary", 182, [8]),
                                Chapter("C", "x", 400, [20])])
    assert [(c.title, c.start, c.refs) for c in out] == [("B", 156, [6, 8]), ("C", 400, [20])]


def test_review_can_only_choose_existing_candidates():
    from lariksa.minutes import Action
    from lariksa.minutes_writer import _PROMPTS, review_actions
    cands = [Action("Run through prototype", "Dimas", "", 10, [3]), Action("Deploy ke production", "Raka", "tanggal 16", 20, [5])]

    def model(system, user, schema):
        assert schema is REVIEW_SCHEMA
        return {"items": [{"id": 1, "verdict": "in_meeting", "owner": "Dimas", "task": "Run through prototype"},
                          {"id": 2, "verdict": "follow_up", "owner": "Raka Aditya Pratama", "task": "Deploy Edu.Platform ke production"},
                          {"id": 2, "verdict": "follow_up", "owner": "", "task": "dup"},
                          {"id": 7, "verdict": "follow_up", "owner": "X", "task": "invented"},
                          {"id": "abc", "verdict": "follow_up", "owner": "", "task": ""}]}

    out = review_actions(cands, model, _PROMPTS["id"], ["Raka Aditya Pratama", "Dimas Arya Putra"])
    assert [(a.owner, a.task, a.due, a.refs) for a in out] == [
        ("Raka Aditya Pratama", "Deploy Edu.Platform ke production", "tanggal 16", [5])]
    assert review_actions([], model, _PROMPTS["id"], []) == []


def test_one_broken_chunk_is_skipped_not_fatal():
    import pytest

    lines = meeting(60)
    model = FakeModel()
    calls = {"n": 0}

    def flaky(system, user, schema):
        calls["n"] += 1
        if schema is ACTIONS_SCHEMA and calls["n"] == 2:  # first chunk's action pass loops
            raise RuntimeError("Model mengembalikan JSON yang tidak valid.")
        return model(system, user, schema)

    m = write_minutes(lines, flaky)
    assert m.title == "Sinkronisasi rilis" and m.actions  # other chunks still produced items

    def broken(system, user, schema):
        raise RuntimeError("Model mengembalikan JSON yang tidak valid.")

    with pytest.raises(RuntimeError):  # every call failing is systematic and must surface
        write_minutes(lines, broken)


def test_failed_review_keeps_grounded_candidates():
    from lariksa.minutes import Action
    from lariksa.minutes_writer import _PROMPTS, review_actions

    def broken(system, user, schema):
        raise RuntimeError("invalid JSON")

    cands = [Action("B", start=20, refs=[2]), Action("A", start=10, refs=[1])]
    assert [a.task for a in review_actions(cands, broken, _PROMPTS["id"], [])] == ["A", "B"]
