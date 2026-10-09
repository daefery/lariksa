from lariksa.transcript import ME, OTHERS, Utterance, as_text, clock, merge_tracks


def u(speaker, start, end, text):
    return Utterance(speaker, start, end, text)


def test_merge_interleaves_by_time():
    mine = [u(ME, 0, 2, "Halo semua"), u(ME, 10, 12, "Oke, saya follow up")]
    theirs = [u(OTHERS, 3, 6, "Pagi, kita mulai ya")]
    merged = merge_tracks(mine, theirs)
    assert [m.text for m in merged] == ["Halo semua", "Pagi, kita mulai ya", "Oke, saya follow up"]


def test_mic_echo_of_others_is_dropped():
    theirs = [u(OTHERS, 5, 9, "Deadline-nya hari Jumat ya, tolong dicek")]
    mine = [u(ME, 5.3, 9.2, "deadline nya hari jumat ya tolong di cek")]
    assert merge_tracks(mine, theirs) == theirs


def test_similar_words_at_a_different_time_are_kept():
    theirs = [u(OTHERS, 5, 9, "Deadline-nya hari Jumat")]
    mine = [u(ME, 20, 23, "Deadline-nya hari Jumat")]
    assert len(merge_tracks(mine, theirs)) == 2


def test_overlapping_but_different_speech_is_kept():
    theirs = [u(OTHERS, 5, 9, "Budget kita tinggal sedikit")]
    mine = [u(ME, 6, 8, "Setuju, saya yang urus")]
    assert len(merge_tracks(mine, theirs)) == 2


def test_as_text_merges_consecutive_turns():
    text = as_text([u(ME, 0, 1, "Satu."), u(ME, 1, 2, "Dua."), u(OTHERS, 65, 66, "Tiga.")])
    assert text == "[00:00] Saya: Satu. Dua.\n[01:05] Peserta: Tiga."


def test_clock_shows_hours_only_when_needed():
    assert clock(59) == "00:59"
    assert clock(3725) == "1:02:05"
