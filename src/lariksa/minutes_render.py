"""Render minutes as Markdown and WhatsApp text, with headings in the minutes' language."""

from __future__ import annotations

from .minutes import Minutes
from .transcript import clock

LABELS = {
    "id": {
        "untitled": "Notulen rapat", "summary": "Ringkasan", "highlights": "Poin penting",
        "actions": "Action items", "decisions": "Keputusan", "questions": "Pertanyaan terbuka",
        "chapters": "Pembahasan", "speakers": "Peserta", "none": "(tidak ada)", "due": "deadline",
        "owner_missing": "Belum ada PIC", "talk": "bicara", "transcript": "Transkrip",
        "task": "Tugas", "owner": "PIC", "due_col": "Deadline", "time": "Waktu",
        "open": "Belum jelas", "unassigned": "Tanpa PIC",
    },
    "en": {
        "untitled": "Meeting notes", "summary": "Summary", "highlights": "Highlights",
        "actions": "Action items", "decisions": "Decisions", "questions": "Open questions",
        "chapters": "Discussion", "speakers": "Speakers", "none": "(none)", "due": "due",
        "owner_missing": "No owner yet", "talk": "talk time", "transcript": "Transcript",
        "task": "Task", "owner": "Owner", "due_col": "Due", "time": "Time",
        "open": "Unclear", "unassigned": "Unassigned",
    },
}


def labels(language: str) -> dict[str, str]:
    return LABELS.get(language, LABELS["id"])


def _owner_due(owner: str, due: str, t: dict) -> str:
    bits = [owner, f"{t['due']}: {due}" if due else ""]
    joined = ", ".join(b for b in bits if b)
    return f" ({joined})" if joined else ""


def to_markdown(m: Minutes, *, date: str = "", title: str = "") -> str:
    t = labels(m.language)
    out = [f"# {title or m.title or t['untitled']}"]
    if date:
        out.append(f"_{date}_")
    out += ["", f"## {t['summary']}", m.overview or "-"]
    if m.highlights:
        out += [""] + [f"- {h}" for h in m.highlights]
    out += ["", f"## {t['actions']}"]
    out += [f"- [{'x' if a.done else ' '}] **{a.owner or t['unassigned']}**: {a.task}"
            + (f" _({t['due']}: {a.due})_" if a.due else "")
            + (f" `{clock(a.start)}`" if a.refs else "") for a in m.actions] or [f"- {t['none']}"]
    out += ["", f"## {t['decisions']}"]
    out += [f"- {d.text}" + (f" `{clock(d.start)}`" if d.refs else "") for d in m.decisions] or [f"- {t['none']}"]
    if m.questions:
        out += ["", f"## {t['questions']}"] + [f"- {q.text}" + (f" `{clock(q.start)}`" if q.refs else "")
                                               for q in m.questions]
    if m.chapters:
        out += ["", f"## {t['chapters']}"]
        for c in m.chapters:
            out += ["", f"### `{clock(c.start)}` {c.title}", c.summary]
    if m.speakers:
        out += ["", f"## {t['speakers']}"] + [f"- {s.name}: {round(s.share * 100)}% {t['talk']}" for s in m.speakers]
    return "\n".join(out) + "\n"


def to_whatsapp(m: Minutes, *, date: str = "", title: str = "") -> str:
    """Short and skimmable: summary, highlights, actions by person, decisions."""
    t = labels(m.language)
    out = [f"*{title or m.title or t['untitled']}*" + (f" - {date}" if date else "")]
    if m.overview:
        out += ["", m.overview]
    if m.highlights:
        out += [""] + [f"• {h}" for h in m.highlights]
    if m.actions:
        out += ["", f"*{t['actions']}*"]
        for owner, items in _by_owner(m).items():
            out.append(f"_{owner or t['unassigned']}_")
            out += [f"• {a.task}" + (f" ({t['due']}: {a.due})" if a.due else "") for a in items]
    if m.decisions:
        out += ["", f"*{t['decisions']}*"] + [f"• {d.text}" for d in m.decisions]
    if m.questions:
        out += ["", f"*{t['questions']}*"] + [f"• {q.text}" for q in m.questions]
    return "\n".join(out).strip() + "\n"


def _by_owner(m: Minutes) -> dict[str, list]:
    groups: dict[str, list] = {}
    for a in m.actions:
        groups.setdefault(a.owner, []).append(a)
    return groups
