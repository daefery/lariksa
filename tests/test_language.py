from lariksa.language import detect, resolve


def test_english_meeting():
    assert detect("Good morning everyone. We will have the meeting today about testing the application.") == "en"


def test_short_english_line():
    # The real 5-second test recording that came out in Indonesian.
    assert detect("Another test for application testing.") == "en"


def test_indonesian_meeting():
    assert detect("Halo semuanya, saya lagi mau mencoba rekaman ini.") == "id"


def test_indonesian_with_english_terms_stays_indonesian():
    assert detect("Jadi kita need to align soal deadline-nya, biar release minggu ini aman.") == "id"


def test_no_signal_uses_default():
    assert detect("", default="en") == "en"
    assert detect("OKR Q4", default="id") == "id"
    assert detect("", default="") == "id"


def test_resolve_fixed_setting_wins():
    english = "We will review the plan and the budget."
    assert resolve("id", english) == "id"
    assert resolve("en", "Kita bahas anggarannya ya.") == "en"
    assert resolve("auto", english) == "en"
    assert resolve("", english) == "en"
