"""Write rich minutes with a small local model.

Two passes, both small enough for a 4B model with a 16k context:

1. **Read** each chunk of numbered transcript lines (~12k characters) and
   extract chapters, action items, decisions and open questions, each with the
   line numbers it came from. Output is capped per chunk so the JSON always
   closes (the 2048-token limit truncated one big merged answer before).
2. **Write** the overview, highlights and title from the chapter summaries
   (not the raw transcript), which is short and fits any meeting length.

Merging, deduplication, owners, timestamps and speaker stats happen in code.
"""

from __future__ import annotations

import logging
import re
from collections.abc import Callable

from .minutes import (
    Action,
    Chapter,
    Line,
    Minutes,
    Point,
    anchor,
    dedupe_actions,
    dedupe_points,
    match_owner,
    parse_refs,
    render_lines,
    speaker_stats,
)

from .notulen import _clean  # placeholder words ("nanti", "TBD", "empty") -> ""

Complete = Callable[[str, str, dict], dict]

log = logging.getLogger(__name__)

#: Characters of numbered transcript per extraction call (~4k tokens of Indonesian).
CHUNK_CHARS = 12_000

_REFS = {"type": "array", "items": {"type": "string"}, "maxItems": 6}
_STR = {"type": "string"}

TOPICS_SCHEMA = {
    "type": "object",
    "properties": {
        "chapters": {"type": "array", "maxItems": 5, "items": {
            "type": "object",
            "properties": {"title": _STR, "summary": _STR, "refs": _REFS},
            "required": ["title", "summary", "refs"]}},
        "decisions": {"type": "array", "maxItems": 4, "items": {
            "type": "object", "properties": {"text": _STR, "refs": _REFS}, "required": ["text", "refs"]}},
        "questions": {"type": "array", "maxItems": 3, "items": {
            "type": "object", "properties": {"text": _STR, "refs": _REFS}, "required": ["text", "refs"]}},
    },
    "required": ["chapters", "decisions", "questions"],
}

ACTIONS_SCHEMA = {
    "type": "object",
    "properties": {
        "actions": {"type": "array", "maxItems": 10, "items": {
            "type": "object",
            "properties": {"quote": _STR,
                           "kind": {"type": "string", "enum": ["will_do", "asked_to_do", "already_done", "discussion"]},
                           "owner": _STR, "task": _STR, "due": _STR, "refs": _REFS},
            "required": ["quote", "kind", "owner", "task", "due", "refs"]}},
    },
    "required": ["actions"],
}

#: Kept for callers and tests that refer to the first pass by its old name.
EXTRACT_SCHEMA = TOPICS_SCHEMA

REVIEW_SCHEMA = {
    "type": "object",
    "properties": {
        "items": {"type": "array", "items": {
            "type": "object",
            "properties": {"id": {"type": "integer"},
                           "verdict": {"type": "string", "enum": ["follow_up", "in_meeting", "demo_feature",
                                                                   "already_answered", "duplicate", "unclear"]},
                           "owner": _STR, "task": _STR},
            "required": ["id", "verdict", "owner", "task"]}},
    },
    "required": ["items"],
}

OVERVIEW_SCHEMA = {
    "type": "object",
    "properties": {
        "title": _STR,
        "overview": _STR,
        "highlights": {"type": "array", "items": _STR, "maxItems": 8},
    },
    "required": ["title", "overview", "highlights"],
}

_PROMPTS = {
    "id": {
        "system": (
            "Kamu notulis rapat yang teliti. Transkrip diberikan per baris bernomor: "
            "'L12 [04:23] Nama: kalimat'. Tulis semua isi dalam Bahasa Indonesia yang rapi; istilah kerja "
            "berbahasa Inggris boleh dipakai. Hanya tulis hal yang benar-benar diucapkan. Setiap butir WAJIB "
            "menyertakan refs: nomor baris sumbernya (misal [\"L12\",\"L13\"]). Transkrip mesin bisa salah "
            "dengar; perbaiki ejaan nama dan istilah yang jelas salah, tapi jangan menebak isi."
        ),
        "extract": (
            "Bagian {index} dari {total} transkrip rapat:\n{text}\n\n"
            "Ambil dari bagian ini saja:\n"
            "- chapters: 2-5 topik berurutan yang mencakup SELURUH bagian ini, dimulai dari baris pertama. "
            "title 3-8 kata; summary 2-3 kalimat yang menyebut siapa menyampaikan apa (pakai nama dari "
            "transkrip), angka, tanggal, dan alasannya.\n"
            "- decisions: hal yang ditetapkan di rapat dan mengubah rencana: pilihan, persetujuan, nama, tanggal, "
            "aturan baru (misal 'Platform dipisah', 'Prototipe dikunci', 'Deploy tanggal 16'). Bukan laporan "
            "progres atau penjelasan.\n"
            "- questions: hanya pertanyaan yang benar-benar diucapkan di rapat dan belum terjawab. "
            "Jangan membuat pertanyaan sendiri.\n"
            "Lebih baik sedikit tapi benar daripada banyak tapi menebak."
        ),
        "actions": (
            "Bagian {index} dari {total} transkrip rapat:\n{text}\n\n"
            "Cari SEMUA tugas tindak lanjut di bagian ini: hal yang seseorang katakan akan dikerjakan, atau "
            "diminta untuk dikerjakan, SETELAH rapat. Untuk tiap kandidat isi:\n"
            "- quote: kutipan pendek kalimat aslinya\n"
            "- kind: will_do (orang itu bilang akan mengerjakan), asked_to_do (orang lain memintanya), "
            "already_done (sudah dikerjakan sebelum/selama rapat), discussion (hanya pendapat, penjelasan, "
            "atau pertanyaan)\n"
            "- owner: nama orang yang akan MENGERJAKAN, sesuai nama di transkrip\n"
            "- task: kalimat perintah singkat diawali kata kerja (Deploy, Siapkan, Cek, Bagikan)\n"
            "- due: persis seperti diucapkan (\"tanggal 16\", \"minggu depan\", \"dalam satu hari\"), "
            "kosongkan jika tidak ada\n"
            "- refs: nomor baris kutipan itu\n"
            "Contoh: 'Nanti aku deploy ke production tanggal 16' -> will_do, owner pembicara, "
            "task 'Deploy ke production', due 'tanggal 16'. 'Tolong Mas Dimas bantu testing ya' -> asked_to_do, "
            "owner Dimas, task 'Bantu testing'. 'Mbak Laras, udah siap jualan? Harus jualan sebelum launch' "
            "-> asked_to_do, owner Laras (orang yang DIAJAK BICARA, bukan pembicara), task 'Mulai jualan "
            "sebelum launch'. 'Kemarin aku sudah fix edge case-nya' -> already_done. 'Prosesnya itu scoring "
            "otomatis' -> discussion. 'Aku share screen dulu ya' atau 'lanjut ke prototipe berikutnya' -> "
            "discussion (terjadi di dalam rapat)."
        ),
        "review": (
            "Ini daftar kandidat tugas dari sebuah rapat, dengan kutipan aslinya:\n{items}\n\n"
            "Beri verdict untuk SETIAP kandidat, semua id:\n"
            "- follow_up: pekerjaan nyata yang harus dikerjakan seseorang SETELAH rapat\n"
            "- in_meeting: hal yang terjadi di dalam rapat (presentasi, run through, share screen, menjelaskan, "
            "'lanjut ke prototipe berikutnya')\n"
            "- demo_feature: fitur produk yang sedang didemokan (misal 'Tambahkan fitur AI...' saat demo prototipe)\n"
            "- already_answered: pertanyaan yang sudah dijawab di rapat ('Tanyakan ke X apakah bisa...' padahal X "
            "sudah menjawab bisa)\n"
            "- duplicate: sama dengan kandidat lain\n"
            "- unclear: tidak jelas siapa atau apa\n"
            "Isi owner dengan nama lengkap orang yang MENGERJAKAN (jika diminta orang lain, owner adalah yang "
            "diminta). Isi task dengan kalimat perintah yang jelas dan berdiri sendiri, menyebut objeknya (misal "
            "'Deploy Edu.Platform ke production', bukan 'Deploy produk'), maksimal 14 kata, tanpa alasan atau "
            "tujuan tambahan. Pertahankan istilah kerja dan nama asli."
        ),
        "overview": (
            "Berikut ringkasan per topik dari satu rapat, berurutan:\n{chapters}\n\n"
            "Keputusan:\n{decisions}\n\n"
            "Tulis:\n- title: judul rapat 4-8 kata\n- overview: 1-2 kalimat tentang inti rapat\n"
            "- highlights: 4-8 poin terpenting, masing-masing satu kalimat pendek (maks 12 kata) yang memuat "
            "angka, tanggal, atau keputusan konkret bila ada. Tanpa nomor baris."
        ),
    },
    "en": {
        "system": (
            "You are a careful meeting note-taker. The transcript is given as numbered lines: "
            "'L12 [04:23] Name: sentence'. Write everything in clear English. Only state what was actually "
            "said. Every item MUST include refs: the source line numbers (e.g. [\"L12\",\"L13\"]). The "
            "transcript is machine-made; fix obvious misspellings of names and terms, but never guess content."
        ),
        "extract": (
            "Part {index} of {total} of the meeting transcript:\n{text}\n\n"
            "From this part only, extract:\n"
            "- chapters: 2-5 topics in order covering ALL of this part, starting from its first line. "
            "title 3-8 words; summary 2-3 sentences naming who said what (use names from the transcript), "
            "with numbers, dates and reasons.\n"
            "- decisions: things settled in the meeting that change the plan: choices, approvals, names, dates, "
            "new rules (e.g. 'Platforms are split', 'Prototype locked', 'Deploy on the 16th'). Not progress "
            "reports or explanations.\n"
            "- questions: only questions actually asked in the meeting and left unanswered. Never invent "
            "questions.\n"
            "Fewer correct items beat many guessed ones."
        ),
        "actions": (
            "Part {index} of {total} of the meeting transcript:\n{text}\n\n"
            "Find ALL follow-up tasks in this part: things someone said they will do, or was asked to do, "
            "AFTER the meeting. For each candidate fill in:\n"
            "- quote: a short quote of the original sentence\n"
            "- kind: will_do (they said they will), asked_to_do (someone asked them), already_done (done "
            "before or during the meeting), discussion (only an opinion, explanation or question)\n"
            "- owner: the name of the person who will DO it, as written in the transcript\n"
            "- task: a short imperative starting with a verb (Deploy, Prepare, Check, Share)\n"
            "- due: exactly as said (\"the 16th\", \"next week\", \"within a day\"), empty if none\n"
            "- refs: the line numbers of that quote\n"
            "Examples: 'I'll deploy to production on the 16th' -> will_do, owner the speaker, task 'Deploy to "
            "production', due 'the 16th'. 'Dimas, can you help with testing?' -> asked_to_do, owner Dimas, task "
            "'Help with testing'. 'Laras, ready to sell? We must sell before launch' -> asked_to_do, owner "
            "Laras (the person SPOKEN TO, not the speaker), task 'Start selling before launch'. 'I already "
            "fixed the edge cases yesterday' -> already_done. 'The process is automated scoring' -> discussion. "
            "'Let me share my screen' or 'moving to the next prototype' -> discussion (happens inside the "
            "meeting)."
        ),
        "review": (
            "Here are candidate tasks from one meeting, with their original quotes:\n{items}\n\n"
            "Give a verdict for EVERY candidate, all ids:\n"
            "- follow_up: real work someone must do AFTER the meeting\n"
            "- in_meeting: something that happened in the meeting (presenting, run-throughs, screen sharing, "
            "explaining, 'moving to the next prototype')\n"
            "- demo_feature: a product feature being demoed (e.g. 'Add AI feature...' during a prototype demo)\n"
            "- already_answered: a question answered in the meeting ('Ask X whether they can...' when X already "
            "said yes)\n"
            "- duplicate: the same as another candidate\n"
            "- unclear: unclear who or what\n"
            "Fill owner with the full name of the person who will DO it (if someone else asked, the owner is the "
            "person asked). Fill task with a clear, self-contained imperative naming its object (e.g. 'Deploy "
            "Edu.Platform to production', not 'Deploy product'), at most 14 words, no added reasons or goals. "
            "Keep the original names and working terms."
        ),
        "overview": (
            "Below are the topic summaries of one meeting, in order:\n{chapters}\n\n"
            "Decisions:\n{decisions}\n\n"
            "Write:\n- title: a 4-8 word meeting title\n- overview: 1-2 sentences on what the meeting was about\n"
            "- highlights: the 4-8 most important points, each one short sentence (max 12 words) with "
            "concrete numbers, dates or decisions where there are any. No line numbers."
        ),
    },
}


def chunk_lines(lines: list[Line], size: int = CHUNK_CHARS) -> list[list[Line]]:
    """Split on line boundaries; never cut a line."""
    out: list[list[Line]] = []
    cur: list[Line] = []
    length = 0
    for line in lines:
        rendered = len(line.render()) + 1
        if cur and length + rendered > size:
            out.append(cur)
            cur, length = [], 0
        cur.append(line)
        length += rendered
    if cur:
        out.append(cur)
    return out


# --- Guardrails ----------------------------------------------------------------
# A 4B model leaks prompt artefacts and pads lists. These checks run on every
# answer, so the page only shows what the transcript supports.

_REF_TEXT = re.compile(r"\s*[\[(]?\bL\d+(?:\s*[-–,]\s*L?\d+)*[\])]?")
_CLOCK = re.compile(r"\b(?:pada\s+|at\s+)?(?:hari|menit|minute|pukul)?\s*\b\d{1,2}:\d{2}(?::\d{2})?\b")
_NUM = re.compile(r"\d+")
_DATE_PHRASE = re.compile(
    r"\b(?:tanggal|tgl\.?)\s*\d{1,2}(?:\s+(?:jan\w*|feb\w*|mar\w*|apr\w*|mei|jun\w*|jul\w*|agu\w*|sep\w*|"
    r"okt\w*|nov\w*|des\w*))?|\b\d{1,2}\s+(?:jan\w*|feb\w*|mar\w*|apr\w*|mei|may|jun\w*|jul\w*|agu\w*|aug\w*|"
    r"sep\w*|okt\w*|oct\w*|nov\w*|des\w*|dec\w*)|\bend of (?:the )?\w+|\bminggu depan\b|\bnext week\b|\bbesok\b|"
    r"\btomorrow\b|\b(?:senin|selasa|rabu|kamis|jumat|sabtu|minggu|monday|tuesday|wednesday|thursday|friday)\b"
    r"|\b(?:dalam\s+)?(?:satu|1)\s+hari\b|\bakhir tahun\b|\bend of (?:the )?year\b", re.I)
#: Things said during the meeting, not work to do after it.
_IN_MEETING = re.compile(
    r"^(?:menjelaskan|melaporkan|menanyakan|mempertanyakan|mengklarifikasi|menyampaikan|mempresentasikan|"
    r"meminta klarifikasi|memberikan apresiasi|menegaskan|menekankan|explain\w*|report(?:ed)?\b|ask(?:ed)? about|"
    r"clarif\w*|present(?:ed)?\b|emphasi[sz]\w*)", re.I)
_QUESTION_CUE = re.compile(r"\?|\b(?:apakah|bagaimana|gimana|kenapa|mengapa|kapan|siapa|belum|nanti dibahas|"
                           r"how|what|why|when|who|not yet)\b", re.I)


def clean_text(text: str) -> str:
    """Remove leaked line refs and transcript clock times; tidy spacing."""
    text = _REF_TEXT.sub("", str(text or ""))
    text = _CLOCK.sub("", text)
    return re.sub(r"\s+([.,;:])", r"\1", " ".join(text.split())).strip(" ,;")


def ground_due(due: str, source: str) -> str:
    """Keep a due date only if its numbers were actually said; else recover the phrase from the source."""
    due = clean_text(due)
    if due and all(n in _NUM.findall(source) for n in _NUM.findall(due)) and _NUM.findall(due) or (
            due and not _NUM.findall(due) and due.lower() in source.lower()):
        return due
    found = _DATE_PHRASE.search(source)
    return found.group(0) if found and due else ""


def _text_of(refs: list[int], lines: list[Line]) -> str:
    by_n = {line.n: line for line in lines}
    return " ".join(by_n[n].text for n in refs if n in by_n)


def _ground(raw: dict, chunk: list[Line], all_lines: list[Line], speakers: list[str]):
    """Turn one model answer into anchored items; drop anything without valid refs in this chunk."""
    chapters, actions, decisions, questions = [], [], [], []
    for c in raw.get("chapters") or []:
        refs = parse_refs(c.get("refs"), chunk)
        title, summary = clean_text(c.get("title")), clean_text(c.get("summary"))
        if refs and title and summary:
            chapters.append(Chapter(title, summary, anchor(refs, all_lines), refs))
    for a in raw.get("actions") or []:
        refs = parse_refs(a.get("refs"), chunk)
        task = clean_text(a.get("task"))
        if not refs or not task or a.get("kind") not in (None, "will_do", "asked_to_do", "follow_up") \
                or _IN_MEETING.match(task):
            continue
        source = _text_of(refs, all_lines)
        action = Action(task, match_owner(_clean(a.get("owner")), speakers),
                        ground_due(_clean(a.get("due")), source), anchor(refs, all_lines), refs)
        action._quote = clean_text(a.get("quote")) or source[:200]  # for the review pass only
        actions.append(action)
    for key, bucket in (("decisions", decisions), ("questions", questions)):
        for p in raw.get(key) or []:
            refs = parse_refs(p.get("refs"), chunk)
            text = clean_text(p.get("text"))
            if not refs or not text:
                continue
            # A question must have been asked: its source lines carry a question cue.
            if key == "questions" and not _QUESTION_CUE.search(_text_of(refs, all_lines)):
                continue
            bucket.append(Point(text, anchor(refs, all_lines), refs))
    return chapters, actions, decisions, questions


def review_actions(actions: list[Action], complete: Complete, prompts: dict, speakers: list[str]) -> list[Action]:
    """Second look at every candidate at once: drop in-meeting chatter, sharpen task wording.

    The review may only choose among existing candidates (by id) and rewrite the
    wording; references, times and due dates stay as grounded in the first pass.
    """
    if not actions:
        return []
    items = "\n".join(
        f"{i}. owner={a.owner or '-'} | task={a.task} | due={a.due or '-'} | kutipan=\"{getattr(a, '_quote', '')}\""
        for i, a in enumerate(actions, start=1))
    try:
        answer = complete(prompts["system"], prompts["review"].format(items=items), REVIEW_SCHEMA)
    except Exception as exc:
        log.warning("Action review skipped (%s); keeping grounded candidates", exc)
        return sorted(actions, key=lambda a: a.start)
    kept: list[Action] = []
    for row in answer.get("items") or answer.get("keep") or []:
        if row.get("verdict", "follow_up") != "follow_up":
            continue
        try:
            index = int(row.get("id")) - 1
        except (TypeError, ValueError):
            continue
        if not 0 <= index < len(actions) or actions[index] in kept:
            continue
        action = actions[index]
        task = clean_text(row.get("task"))
        # Keep the review's wording only when it stays short; padded rewrites lose to the original.
        if task and not _IN_MEETING.match(task) and len(task.split()) <= 18:
            action.task = task
        owner = match_owner(_clean(row.get("owner")), speakers)
        if owner:
            action.owner = owner
        kept.append(action)
    return sorted(kept, key=lambda a: a.start)


def merge_close_chapters(chapters: list[Chapter], gap: float = 45.0) -> list[Chapter]:
    """Chapters that start within ``gap`` seconds of each other are one topic split in two."""
    out: list[Chapter] = []
    for c in chapters:
        if out and c.start - out[-1].start < gap:
            prev = out[-1]
            prev.refs = sorted(set(prev.refs) | set(c.refs))
            if len(c.summary) > len(prev.summary):
                prev.title, prev.summary = c.title, c.summary
        else:
            out.append(c)
    return out


def write_minutes(
    lines: list[Line],
    complete: Complete,
    *,
    language: str = "id",
    on_progress: Callable[[float], None] | None = None,
) -> Minutes:
    p = _PROMPTS.get(language, _PROMPTS["id"])
    speakers = [s.name for s in speaker_stats(lines)]
    chunks = chunk_lines(lines)
    steps = 2 * len(chunks) + 2

    chapters, actions, decisions, questions = [], [], [], []
    failures = 0

    def attempt(user: str, schema: dict, empty: dict) -> dict:
        """One chunk's answer; a failed chunk is skipped, not fatal, unless every chunk fails."""
        nonlocal failures
        try:
            return complete(p["system"], user, schema)
        except Exception as exc:  # LLMError: invalid JSON after retries
            failures += 1
            log.warning("Minutes chunk skipped (%s)", exc)
            if failures >= max(2, len(chunks)):  # systematic, not a one-off loop
                raise
            return empty

    for index, chunk in enumerate(chunks, start=1):
        text = render_lines(chunk)
        topics = attempt(p["extract"].format(index=index, total=len(chunks), text=text), TOPICS_SCHEMA,
                         {"chapters": [], "decisions": [], "questions": []})
        if on_progress:
            on_progress((2 * index - 1) / steps)
        todo = attempt(p["actions"].format(index=index, total=len(chunks), text=text), ACTIONS_SCHEMA,
                       {"actions": []})
        c, a, d, q = _ground({**topics, "actions": todo.get("actions")}, chunk, lines, speakers)
        chapters += c
        actions += a
        decisions += d
        questions += q
        if on_progress:
            on_progress(2 * index / steps)

    chapters = merge_close_chapters(sorted(chapters, key=lambda c: c.start))
    actions = review_actions(dedupe_actions(actions), complete, p, speakers)
    decisions = dedupe_points(decisions)
    questions = dedupe_points(questions)

    chapter_text = "\n".join(f"[{_mmss(c.start)}] {c.title}: {c.summary}" for c in chapters) or "-"
    decision_text = "\n".join(f"- {d.text}" for d in decisions) or "-"
    head = complete(p["system"], p["overview"].format(chapters=chapter_text, decisions=decision_text),
                    OVERVIEW_SCHEMA) if chapters else {}
    if on_progress:
        on_progress(1.0)

    return Minutes(
        title=clean_text(head.get("title")),
        overview=clean_text(head.get("overview")),
        highlights=[h for h in (clean_text(x) for x in head.get("highlights") or []) if h][:8],
        chapters=chapters,
        actions=actions,
        decisions=decisions,
        questions=questions,
        speakers=speaker_stats(lines),
        language=language,
    )


def _mmss(seconds: float) -> str:
    from .transcript import clock
    return clock(seconds)
