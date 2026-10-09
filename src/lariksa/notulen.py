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

    def to_dict(self) -> dict:
        return asdict(self)

    @classmethod
    def from_dict(cls, data: dict) -> Notulen:
        items = [ActionItem(**_pick(i, ActionItem)) for i in data.get("action_items") or []
                 if isinstance(i, dict) and i.get("task")]
        return cls(
            title=str(data.get("title") or "").strip(),
            summary=str(data.get("summary") or "").strip(),
            topics=_strings(data.get("topics")),
            decisions=_strings(data.get("decisions")),
            action_items=items,
            open_questions=_strings(data.get("open_questions")),
        )


def _strings(value) -> list[str]:
    return [str(v).strip() for v in value or [] if str(v).strip()]


def _pick(data: dict, cls) -> dict:
    names = cls.__dataclass_fields__
    return {k: str(v or "").strip() for k, v in data.items() if k in names}


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

_LANG = {"id": "Bahasa Indonesia", "en": "English"}


def _system(language: str) -> str:
    lang = _LANG.get(language, "Bahasa Indonesia")
    return (
        "Kamu notulis rapat yang teliti. Tulis dalam "
        f"{lang} yang rapi dan ringkas; istilah bahasa Inggris yang umum di kantor boleh dipakai. "
        "Hanya pakai fakta yang ada di transkrip; jangan mengarang nama, tanggal, atau angka. "
        "'Saya' adalah pemilik rekaman; 'Peserta' adalah peserta lain. "
        "Jika nama orang disebut di transkrip, pakai nama itu sebagai PIC; jika tidak jelas, isi PIC dengan string kosong. "
        "Deadline hanya diisi jika hari atau tanggal disebut, tulis persis seperti diucapkan "
        "(misal 'Jumat', '15 Okt'); jika tidak disebut, isi string kosong, jangan 'nanti' atau 'segera'. "
        "Transkrip hasil mesin bisa salah dengar; perbaiki ejaan yang jelas salah, tapi jangan menebak isi."
    )


def _task(context: str) -> str:
    return (
        f"{context}\n\n"
        "Isi JSON berikut:\n"
        "- title: judul rapat singkat (maks 8 kata) berdasarkan isi\n"
        "- summary: ringkasan 3-5 kalimat\n"
        "- topics: poin-poin pembahasan utama\n"
        "- decisions: keputusan yang disepakati (kosong jika tidak ada)\n"
        "- action_items: tugas konkret dengan task, owner (PIC), due (deadline)\n"
        "- open_questions: hal yang belum terjawab atau perlu dibahas lagi"
    )


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
    """Summarise in one pass, or map-reduce when the meeting is long."""
    system = _system(language)
    hint = f"Judul dari pengguna: {meeting_title}\n" if meeting_title else ""

    if len(transcript_text) <= SINGLE_PASS_CHARS:
        data = complete(system, _task(f"{hint}Transkrip rapat:\n{transcript_text}"), SCHEMA)
        if on_progress:
            on_progress(1.0)
        return Notulen.from_dict(data)

    parts = chunks(transcript_text)
    partials: list[Notulen] = []
    for index, part in enumerate(parts, start=1):
        context = f"Ini bagian {index} dari {len(parts)} transkrip rapat:\n{part}"
        partials.append(Notulen.from_dict(complete(system, _task(context), SCHEMA)))
        if on_progress:
            on_progress(index / (len(parts) + 1))

    notes = json.dumps([p.to_dict() for p in partials], ensure_ascii=False, indent=1)
    merged = complete(
        system,
        _task(
            f"{hint}Berikut catatan per bagian dari satu rapat yang sama, berurutan. "
            "Gabungkan menjadi satu notulen; hapus duplikat dan satukan action item yang sama:\n"
            f"{notes}"
        ),
        SCHEMA,
    )
    if on_progress:
        on_progress(1.0)
    return Notulen.from_dict(merged)


# --------------------------------------------------------------------------
# Rendering
# --------------------------------------------------------------------------

def _bullets(items: list[str]) -> list[str]:
    return [f"- {i}" for i in items] or ["- (tidak ada)"]


def _owner_due(item: ActionItem) -> str:
    bits = [f"PIC: {item.owner}" if item.owner else "", f"deadline: {item.due}" if item.due else ""]
    joined = ", ".join(b for b in bits if b)
    return f" ({joined})" if joined else ""


def to_markdown(n: Notulen, *, date: str = "") -> str:
    lines = [f"# {n.title or 'Notulen rapat'}"]
    if date:
        lines.append(f"_{date}_")
    lines += ["", "## Ringkasan", n.summary or "-", "", "## Pembahasan", *_bullets(n.topics),
              "", "## Keputusan", *_bullets(n.decisions), "", "## Action items"]
    if n.action_items:
        lines += [f"- [ ] {i.task}{_owner_due(i)}" for i in n.action_items]
    else:
        lines.append("- (tidak ada)")
    lines += ["", "## Pertanyaan terbuka", *_bullets(n.open_questions)]
    return "\n".join(lines) + "\n"


def to_whatsapp(n: Notulen, *, date: str = "") -> str:
    """WhatsApp formatting: *bold*, no headings, short."""
    lines = [f"*{n.title or 'Notulen rapat'}*" + (f" - {date}" if date else ""), "", n.summary]
    if n.decisions:
        lines += ["", "*Keputusan*", *[f"• {d}" for d in n.decisions]]
    if n.action_items:
        lines += ["", "*Action items*",
                  *[f"• {i.task}{_owner_due(i)}" for i in n.action_items]]
    if n.open_questions:
        lines += ["", "*Belum terjawab*", *[f"• {q}" for q in n.open_questions]]
    return "\n".join(lines).strip() + "\n"
