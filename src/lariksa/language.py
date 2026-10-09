"""Which language the notulen should be written in.

The notulen follows what was actually said, not the transcription setting:
Whisper writes English speech as English even when told to expect Indonesian,
so the transcript text is the reliable signal.

Detection counts function words, which carry the grammar of a sentence. That
handles the common office case well: "Jadi kita need to align soal
deadline-nya" is Indonesian with English nouns, and its function words (jadi,
kita, soal, nya) are Indonesian, so the notulen is written in Indonesian.
"""

from __future__ import annotations

import re

SUPPORTED = ("id", "en")
AUTO = "auto"

_ID = frozenset(
    "yang dan di ini itu untuk dengan tidak ada kita saya ke dari akan ya gitu nggak gak enggak "
    "aja saja sudah udah bisa juga karena kalau kalo jadi tapi atau sama lagi mau harus belum "
    "kami kamu mereka dia apa bagaimana gimana kenapa sih kan dong nya terus soal biar supaya "
    "nanti sekarang masih lebih sangat banyak semua tolong baik".split()
)
_EN = frozenset(
    "the and to of is are we you that it for in on this be will let's with have so was what how "
    "can do not but they our just i it's there would should could about from been were has "
    "please need let us".split()
)
_WORD = re.compile(r"[a-z']+")


def detect(text: str, default: str = "id") -> str:
    """'en' or 'id' by function-word count; ``default`` when there is no signal.

    Pass spoken text only, without speaker labels ("Saya:" would count as
    Indonesian on every line).
    """
    words = _WORD.findall(text.lower())
    indonesian = sum(w in _ID for w in words)
    english = sum(w in _EN for w in words)
    if english > indonesian:
        return "en"
    if indonesian > english:
        return "id"
    return default if default in SUPPORTED else "id"


def resolve(setting: str, spoken_text: str, fallback: str = "id") -> str:
    """The notulen language: a fixed choice, or detected from the transcript."""
    if setting in SUPPORTED:
        return setting
    return detect(spoken_text, default=fallback)
