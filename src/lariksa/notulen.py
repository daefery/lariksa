"""Turn a transcript into notulen: summary, decisions, action items with PIC.

The model only fills a fixed JSON shape (enforced by llama.cpp's grammar), and
this module owns everything around it: chunking long meetings, merging the
chunk notes, and rendering the result. That keeps a 4B model reliable: it never
has to remember formatting, only read and extract.
"""

from __future__ import annotations

import json
from collections.abc import Callable
from dataclasses import asdict, dataclass, field

#: A ~1 hour meeting is roughly 45-55k characters of Indonesian, about 15k
#: tokens. Single-pass up to this size; above it, summarise in chunks first.
SINGLE_PASS_CHARS = 36_000
CHUNK_CHARS = 18_000

#: (system prompt, user prompt, JSON schema) -> parsed JSON
Complete = Callable[[str, str, dict], dict]


@dataclass
class ActionItem:
    task: str
    owner: str = ""
    due: str = ""


@dataclass
class Notulen:
    title: str = ""
    summary: str = ""
    topics: list[str] = field(default_factory=list)
    decisions: list[str] = field(default_factory=list)
    action_items: list[ActionItem] = field(default_factory=list)
    open_questions: list[str] = field(default_factory=list)
    #: Language the notulen is written in; renderers use it for headings.
    language: str = "id"

    def to_dict(self) -> dict:
        return asdict(self)

    @classmethod
    def from_dict(cls, data: dict, language: str | None = None) -> Notulen:
        """``language`` overrides the stored one (the model's JSON has none)."""
        items = [ActionItem(**_pick(i, ActionItem)) for i in data.get("action_items") or []
                 if isinstance(i, dict) and i.get("task")]
        return cls(
            title=str(data.get("title") or "").strip(),
            summary=str(data.get("summary") or "").strip(),
            topics=_strings(data.get("topics")),
            decisions=_strings(data.get("decisions")),
            action_items=items,
            open_questions=_strings(data.get("open_questions")),
            language=language or (data.get("language") if data.get("language") in LABELS else "id"),
        )


def _strings(value) -> list[str]:
    return [str(v).strip() for v in value or [] if str(v).strip()]


#: Words a small model writes instead of leaving owner/due blank. Prompts ask for
#: an empty string, but it still answers "empty", "nanti", "TBD"... so they are
#: cleared here, whatever the language.
_PLACEHOLDERS = frozenset({
    "", "-", "--", "?", "empty", "none", "null", "n/a", "na", "tbd", "tba", "unknown",
    "later", "soon", "asap", "not specified", "unspecified", "not mentioned",
    "nanti", "segera", "tidak ada", "belum ada", "tidak disebut", "belum ditentukan", "kosong",
})


def _clean(value) -> str:
    text = str(value or "").strip()
    return "" if text.lower().strip(" .()") in _PLACEHOLDERS else text


def _pick(data: dict, cls) -> dict:
    names = cls.__dataclass_fields__
    return {k: _clean(v) for k, v in data.items() if k in names}


_LIST = {"type": "array", "items": {"type": "string"}}
SCHEMA = {
    "type": "object",
    "properties": {
        "title": {"type": "string"},
        "summary": {"type": "string"},
        "topics": _LIST,
        "decisions": _LIST,
        "action_items": {
            "type": "array",
            "items": {
                "type": "object",
                "properties": {
                    "task": {"type": "string"},
                    "owner": {"type": "string"},
                    "due": {"type": "string"},
                },
                "required": ["task", "owner", "due"],
            },
        },
        "open_questions": _LIST,
    },
    "required": ["title", "summary", "topics", "decisions", "action_items", "open_questions"],
}

# The whole prompt is written in the output language: a 4B model follows the
# language of its instructions more reliably than an instruction to switch
# (an Indonesian prompt that said "write in English" still produced Indonesian).
_PROMPTS = {
    "id": {
        "system": (
            "Kamu notulis rapat yang teliti. Tulis seluruh isi JSON dalam Bahasa Indonesia yang rapi "
            "dan ringkas; istilah bahasa Inggris yang umum di kantor boleh dipakai. "
            "Hanya pakai fakta yang ada di transkrip; jangan mengarang nama, tanggal, atau angka. "
            "'Saya' adalah pemilik rekaman; 'Peserta' adalah peserta lain. "
            "Jika nama orang disebut di transkrip, pakai nama itu sebagai PIC; jika tidak jelas, "
            "isi PIC dengan string kosong. "
            "Deadline hanya diisi jika hari atau tanggal disebut, tulis persis seperti diucapkan "
            "(misal 'Jumat', '15 Okt'); jika tidak disebut, isi string kosong, jangan 'nanti' atau 'segera'. "
            "Transkrip hasil mesin bisa salah dengar; perbaiki ejaan yang jelas salah, tapi jangan menebak isi."
        ),
        "fields": (
            "Isi JSON berikut:\n"
            "- title: judul rapat singkat (maks 8 kata) berdasarkan isi\n"
            "- summary: ringkasan 3-5 kalimat\n"
            "- topics: poin-poin pembahasan utama\n"
            "- decisions: keputusan yang disepakati (kosong jika tidak ada)\n"
            "- action_items: tugas konkret dengan task, owner (PIC), due (deadline)\n"
            "- open_questions: hal yang belum terjawab atau perlu dibahas lagi"
        ),
        "title_hint": "Judul dari pengguna: {title}\n",
        "transcript": "Transkrip rapat:\n{text}",
        "part": "Ini bagian {index} dari {total} transkrip rapat:\n{text}",
        "merge": (
            "Berikut catatan per bagian dari satu rapat yang sama, berurutan. "
            "Gabungkan menjadi satu notulen; hapus duplikat dan satukan action item yang sama:\n{notes}"
        ),
    },
    "en": {
        "system": (
            "You are a careful meeting note-taker. Write every JSON value in clear, concise English. "
            "Use only facts stated in the transcript; never invent names, dates or numbers. "
            "'Saya' is the person who recorded the meeting; 'Peserta' are the other participants. "
            "If a person's name is mentioned, use it as the owner; if unclear, leave owner as an empty string. "
            "Fill 'due' only when a day or date is said, written as spoken (e.g. 'Friday', 'Oct 15'); "
            "otherwise use \"\" (an empty string), never words like 'empty', 'later' or 'soon'. "
            "The transcript is machine-generated and may mishear words; fix obvious misspellings, "
            "but do not guess content."
        ),
        "fields": (
            "Fill in this JSON:\n"
            "- title: short meeting title (max 8 words) based on the content\n"
            "- summary: a 3-5 sentence summary\n"
            "- topics: main discussion points\n"
            "- decisions: decisions that were agreed (empty if none)\n"
            "- action_items: concrete tasks with task, owner, due (deadline)\n"
            "- open_questions: things left unanswered or to revisit"
        ),
        "title_hint": "Title given by the user: {title}\n",
        "transcript": "Meeting transcript:\n{text}",
        "part": "This is part {index} of {total} of the meeting transcript:\n{text}",
        "merge": (
            "Below are notes for consecutive parts of the same meeting. "
            "Merge them into one set of minutes; remove duplicates and combine identical action items:\n{notes}"
        ),
    },
}

#: Every user-visible heading and label, per language, shared by all renderers
#: (screen, Markdown, WhatsApp, DOCX) so they can never disagree.
LABELS = {
    "id": {
        "untitled": "Notulen rapat", "summary": "Ringkasan", "topics": "Pembahasan",
        "decisions": "Keputusan", "action_items": "Action items", "open_questions": "Pertanyaan terbuka",
        "open_questions_short": "Belum terjawab", "none": "(tidak ada)", "task": "Tugas", "owner": "PIC",
        "due": "Deadline", "due_inline": "deadline", "transcript": "Transkrip",
    },
    "en": {
        "untitled": "Meeting notes", "summary": "Summary", "topics": "Discussion",
        "decisions": "Decisions", "action_items": "Action items", "open_questions": "Open questions",
        "open_questions_short": "Open questions", "none": "(none)", "task": "Task", "owner": "Owner",
        "due": "Due", "due_inline": "due", "transcript": "Transcript",
    },
}


def labels(language: str) -> dict[str, str]:
    return LABELS.get(language, LABELS["id"])


def _prompts(language: str) -> dict[str, str]:
    return _PROMPTS.get(language, _PROMPTS["id"])


def chunks(text: str, size: int = CHUNK_CHARS) -> list[str]:
    """Split on line boundaries so no speaker turn is cut mid-sentence."""
    parts: list[str] = []
    current: list[str] = []
    length = 0
    for line in text.splitlines():
        if current and length + len(line) > size:
            parts.append("\n".join(current))
            current, length = [], 0
        current.append(line)
        length += len(line) + 1
    if current:
        parts.append("\n".join(current))
    return parts


def write_notulen(
    transcript_text: str,
    complete: Complete,
    *,
    language: str = "id",
    meeting_title: str = "",
    on_progress: Callable[[float], None] | None = None,
) -> Notulen:
    """Summarise in one pass, or map-reduce when the meeting is long.

    ``language`` is the language of the notulen ("id" or "en"), not of the audio.
    """
    p = _prompts(language)
    system = p["system"]
    hint = p["title_hint"].format(title=meeting_title) if meeting_title else ""

    def task(context: str) -> str:
        return f"{context}\n\n{p['fields']}"

    if len(transcript_text) <= SINGLE_PASS_CHARS:
        data = complete(system, task(hint + p["transcript"].format(text=transcript_text)), SCHEMA)
        if on_progress:
            on_progress(1.0)
        return Notulen.from_dict(data, language)

    parts = chunks(transcript_text)
    partials: list[Notulen] = []
    for index, part in enumerate(parts, start=1):
        context = p["part"].format(index=index, total=len(parts), text=part)
        partials.append(Notulen.from_dict(complete(system, task(context), SCHEMA), language))
        if on_progress:
            on_progress(index / (len(parts) + 1))

    notes = json.dumps([{k: v for k, v in n.to_dict().items() if k != "language"} for n in partials],
                       ensure_ascii=False, indent=1)
    merged = complete(system, task(hint + p["merge"].format(notes=notes)), SCHEMA)
    if on_progress:
        on_progress(1.0)
    return Notulen.from_dict(merged, language)


# --------------------------------------------------------------------------
# Rendering
# --------------------------------------------------------------------------

def _bullets(items: list[str], none: str) -> list[str]:
    return [f"- {i}" for i in items] or [f"- {none}"]


def owner_due(item: ActionItem, language: str = "id") -> str:
    t = labels(language)
    bits = [f"{t['owner']}: {item.owner}" if item.owner else "",
            f"{t['due_inline']}: {item.due}" if item.due else ""]
    joined = ", ".join(b for b in bits if b)
    return f" ({joined})" if joined else ""


def to_markdown(n: Notulen, *, date: str = "") -> str:
    language = n.language
    t = labels(language)
    lines = [f"# {n.title or t['untitled']}"]
    if date:
        lines.append(f"_{date}_")
    lines += ["", f"## {t['summary']}", n.summary or "-",
              "", f"## {t['topics']}", *_bullets(n.topics, t["none"]),
              "", f"## {t['decisions']}", *_bullets(n.decisions, t["none"]),
              "", f"## {t['action_items']}"]
    if n.action_items:
        lines += [f"- [ ] {i.task}{owner_due(i, language)}" for i in n.action_items]
    else:
        lines.append(f"- {t['none']}")
    lines += ["", f"## {t['open_questions']}", *_bullets(n.open_questions, t["none"])]
    return "\n".join(lines) + "\n"


def to_whatsapp(n: Notulen, *, date: str = "") -> str:
    """WhatsApp formatting: *bold*, no headings, short."""
    language = n.language
    t = labels(language)
    lines = [f"*{n.title or t['untitled']}*" + (f" - {date}" if date else ""), "", n.summary]
    if n.decisions:
        lines += ["", f"*{t['decisions']}*", *[f"• {d}" for d in n.decisions]]
    if n.action_items:
        lines += ["", f"*{t['action_items']}*",
                  *[f"• {i.task}{owner_due(i, language)}" for i in n.action_items]]
    if n.open_questions:
        lines += ["", f"*{t['open_questions_short']}*", *[f"• {q}" for q in n.open_questions]]
    return "\n".join(lines).strip() + "\n"
