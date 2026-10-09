from lariksa.transcript_import import parse

READAI = """0:00 - Raka Aditya Pratama
So first, a bit of context of what is the project that I built.

 Ternyata sudah pakai kahut.

2:08 - Nadia Kusuma
But also, kalau pakai kahut kan pakai produk lain, gimana?

1:20:08 - Unidentified Speaker
Engineer atau gimana?
"""


def test_readai_headers_with_multi_paragraph_bodies():
    u = parse(READAI)
    assert [(x.speaker, x.start, x.end) for x in u] == [
        ("Raka Aditya Pratama", 0, 128), ("Nadia Kusuma", 128, 4808), ("Unidentified Speaker", 4808, 4818)]
    assert u[0].text == "So first, a bit of context of what is the project that I built. Ternyata sudah pakai kahut."


def test_windows_line_endings_and_bom():
    u = parse("\ufeff" + READAI.replace("\n", "\r\n"))
    assert len(u) == 3 and u[1].speaker == "Nadia Kusuma"


def test_inline_turns():
    u = parse("[00:00:05] Budi: Halo semua\n[00:01:10] Sari: Pagi, kita mulai")
    assert [(x.speaker, x.start, x.text) for x in u] == [("Budi", 5, "Halo semua"), ("Sari", 70, "Pagi, kita mulai")]


def test_webvtt_with_voices():
    vtt = """WEBVTT

00:00:01.000 --> 00:00:04.500
<v Nadia Kusuma>Oke kita mulai.</v>

00:00:05.000 --> 00:00:07.000
Raka: Siap, Bu.
"""
    u = parse(vtt)
    assert [(x.speaker, x.start, x.end, x.text) for x in u] == [
        ("Nadia Kusuma", 1, 4.5, "Oke kita mulai."), ("Raka", 5, 7, "Siap, Bu.")]


def test_plain_text_falls_back_to_one_speaker():
    u = parse("Catatan rapat tanpa waktu.\nBaris kedua.")
    assert len(u) == 1 and u[0].text == "Catatan rapat tanpa waktu. Baris kedua."
    assert parse("   ") == []
    assert parse("WEBVTT\n\n") == []  # a subtitle file without cues is empty, not one "WEBVTT" line
