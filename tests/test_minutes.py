from lariksa.minutes import (
    Action,
    Line,
    Minutes,
    Point,
    dedupe_actions,
    dedupe_points,
    is_rich,
    lines_from,
    match_owner,
    parse_refs,
    render_lines,
    speaker_stats,
)
from lariksa.transcript import Utterance

SPEAKERS = ["Raka Aditya Pratama", "Nadia Kusuma", "Bima Satria Wijaya", "Unidentified Speaker"]


def test_lines_merge_same_speaker_and_number_them():
    lines = lines_from([
        Utterance("Nadia", 0, 2, "Oke kita mulai."),
        Utterance("Nadia", 2.5, 4, "Agenda pertama rilis."),
        Utterance("Raka", 5, 7, "Siap."),
    ])
    assert [(l.n, l.speaker, l.text) for l in lines] == [
        (1, "Nadia", "Oke kita mulai. Agenda pertama rilis."), (2, "Raka", "Siap.")]
    assert render_lines(lines).splitlines()[1] == "L2 [00:05] Raka: Siap."


def test_long_monologue_splits_at_sentences_with_increasing_times():
    text = " ".join(f"Kalimat nomor {i} cukup panjang untuk menguji pemotongan baris." for i in range(30))
    lines = lines_from([Utterance("Nadia", 100, 400, text)])
    assert len(lines) > 3
    assert all(len(l.text) <= 420 for l in lines)
    assert [l.start for l in lines] == sorted(l.start for l in lines)
    assert lines[0].start == 100 and lines[-1].end <= 400.01
    assert " ".join(l.text for l in lines) == text


def test_parse_refs_accepts_messy_model_output_and_drops_invented_lines():
    lines = [Line(n, n * 10, n * 10 + 5, "A", "x") for n in range(1, 21)]
    assert parse_refs(["L3", 5, "L7-L9", "L99", "foo", "L3"], lines) == [3, 5, 7, 8, 9]
    assert parse_refs("L12", lines) == [12]
    assert parse_refs(None, lines) == []
    assert parse_refs(["L1-L900"], lines) == [1]  # absurd ranges are not expanded


def test_match_owner_to_speakers():
    assert match_owner("Raka", SPEAKERS) == "Raka Aditya Pratama"
    assert match_owner("bima", SPEAKERS) == "Bima Satria Wijaya"
    assert match_owner("Nadia Kusuma", SPEAKERS) == "Nadia Kusuma"
    assert match_owner("Melati", SPEAKERS) == "Melati"  # mentioned, not speaking: kept
    assert match_owner("  ", SPEAKERS) == ""
    # Two-track labels never absorb a name.
    assert match_owner("Saya", ["Saya", "Peserta"]) == "Saya"
    assert match_owner("Budi", ["Saya", "Peserta"]) == "Budi"


def test_speaker_stats_share_and_turns():
    lines = [Line(1, 0, 60, "A", "x"), Line(2, 60, 80, "B", "y"), Line(3, 80, 100, "A", "z")]
    stats = speaker_stats(lines)
    assert [(s.name, s.seconds, s.turns) for s in stats] == [("A", 80, 2), ("B", 20, 1)]
    assert abs(sum(s.share for s in stats) - 1) < 0.01


def test_dedupe_actions_merges_same_task_from_two_chunks():
    a = Action("Deploy Edu.Platform ke production tanggal 16", "Raka Aditya Pratama", "16 Okt", 1000, [40])
    b = Action("Deploy Edu.Platform ke production pada tanggal 16", "Raka Aditya Pratama", "", 1100, [44])
    c = Action("Siapkan strategi re-engagement 180.000 pengguna", "Tri Samsul", "", 2500, [90])
    out = dedupe_actions([a, b, c])
    assert len(out) == 2
    assert out[0].refs == [40, 44] and out[0].due == "16 Okt"


def test_dedupe_points():
    out = dedupe_points([Point("Prototype Community dikunci", 10, [5]), Point("Prototype community siap dikunci", 20, [9])])
    assert len(out) == 1 and out[0].refs == [5, 9]


def test_roundtrip_and_version():
    m = Minutes(title="T", actions=[Action("x", refs=[1])], language="en")
    back = Minutes.from_dict(m.to_dict())
    assert back == m and is_rich(m.to_dict())
    assert not is_rich({"title": "old", "summary": "v1 notulen"})
    # An upgraded v1 notulen saved back keeps loading as v2 (not re-read as legacy and emptied).
    from lariksa.minutes import from_legacy, load_minutes
    up = from_legacy({"title": "old", "summary": "s", "action_items": [{"task": "Do", "owner": "A"}]})
    assert up.version == 1 and load_minutes(up.to_dict()).actions[0].task == "Do"
