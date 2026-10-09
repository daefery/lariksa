"""Exports: DOCX for the office, Markdown with the full transcript.

Headings follow the minutes' language (see ``minutes_render.LABELS``).
"""

from __future__ import annotations

from pathlib import Path

from .minutes_render import labels, to_markdown
from .store import Meeting
from .transcript import clock

_FOREST = (0x23, 0x36, 0x2C)
_MUTED = (0x65, 0x73, 0x6B)


def _language(meeting: Meeting) -> str:
    return meeting.notulen.language if meeting.notulen else "id"


def full_markdown(meeting: Meeting, *, date: str = "") -> str:
    t = labels(_language(meeting))
    parts = []
    if meeting.notulen:
        parts.append(to_markdown(meeting.notulen, date=date, title=meeting.title))
    else:
        parts.append(f"# {meeting.title}\n_{date}_\n")
    if meeting.transcript:
        parts.append(f"## {t['transcript']}\n")
        parts += [f"**[{clock(u.start)}] {u.speaker}:** {u.text}  " for u in meeting.transcript]
    return "\n".join(parts) + "\n"


def write_docx(meeting: Meeting, target: Path, *, date: str = "", with_transcript: bool = True) -> None:
    from docx import Document
    from docx.shared import Pt, RGBColor

    m = meeting.notulen
    t = labels(_language(meeting))
    doc = Document()
    style = doc.styles["Normal"]
    style.font.name = "Calibri"
    style.font.size = Pt(11)

    def heading(text: str, level: int) -> None:
        h = doc.add_heading(text, level=level)
        for run in h.runs:
            run.font.color.rgb = RGBColor(*_FOREST)

    def muted(paragraph, text: str) -> None:
        run = paragraph.add_run(text)
        run.font.color.rgb = RGBColor(*_MUTED)

    def bullets(items: list[str]) -> None:
        for item in items or [t["none"]]:
            doc.add_paragraph(item, style="List Bullet")

    heading(meeting.title or m.title or t["untitled"], 0)
    if date:
        muted(doc.add_paragraph(), date)

    heading(t["summary"], 1)
    doc.add_paragraph(m.overview or "-")
    if m.highlights:
        bullets(m.highlights)

    heading(t["actions"], 1)
    if m.actions:
        table = doc.add_table(rows=1, cols=4)
        table.style = "Light Grid Accent 1"
        for cell, text in zip(table.rows[0].cells, (t["owner"], t["task"], t["due_col"], t["time"]), strict=True):
            cell.text = text
        for a in m.actions:
            row = table.add_row().cells
            row[0].text = a.owner or "-"
            row[1].text = ("✓ " if a.done else "") + a.task
            row[2].text = a.due or "-"
            row[3].text = clock(a.start) if a.refs else "-"
    else:
        doc.add_paragraph(t["none"])

    heading(t["decisions"], 1)
    bullets([d.text for d in m.decisions])
    if m.questions:
        heading(t["questions"], 1)
        bullets([q.text for q in m.questions])

    if m.chapters:
        heading(t["chapters"], 1)
        for c in m.chapters:
            p = doc.add_paragraph()
            muted(p, f"{clock(c.start)}  ")
            p.add_run(c.title).bold = True
            doc.add_paragraph(c.summary)

    if m.speakers:
        heading(t["speakers"], 1)
        bullets([f"{s.name}: {round(s.share * 100)}% {t['talk']}" for s in m.speakers])

    if with_transcript and meeting.transcript:
        doc.add_page_break()
        heading(t["transcript"], 1)
        for u in meeting.transcript:
            p = doc.add_paragraph()
            label = p.add_run(f"[{clock(u.start)}] {u.speaker}: ")
            label.bold = True
            label.font.color.rgb = RGBColor(*_MUTED)
            p.add_run(u.text)

    doc.save(str(target))
