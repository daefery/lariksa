"""Rich meeting minutes: chapters, action items and decisions anchored to the transcript.

Everything the model writes must point back to evidence. Each chapter, action
item, decision and open question carries the transcript lines it came from
(``refs``); the start time, the speaker label and the "jump to" link are
derived from those lines here, in code, never typed by the model. A claim with
no valid reference is dropped, so nothing ungrounded reaches the page.

The model sees the transcript as numbered lines (``L12 [04:23] Nadia: ...``)
and answers with line numbers. That is the whole trick that lets a 4B local
model produce Read.ai-grade structure: it never has to copy timestamps or
names, it only has to read and point.
"""

from __future__ import annotations

import re
from collections import Counter
from dataclasses import asdict, dataclass, field

from .transcript import Utterance, clock

# ---------------------------------------------------------------------------
# Line view of a transcript
# ---------------------------------------------------------------------------

#: Turns longer than this are split at sentence ends so a reference can point
#: to the exact moment, not the start of a five-minute monologue.
MAX_LINE_CHARS = 420
_SENTENCE = re.compile(r"(?<=[.!?…])\s+")


@dataclass(frozen=True)
class Line:
    n: int
    start: float
    end: float
    speaker: str
    text: str

    def render(self) -> str:
        return f"L{self.n} [{clock(self.start)}] {self.speaker}: {self.text}"


def lines_from(utterances: list[Utterance]) -> list[Line]:
    """Merge same-speaker utterances into turns, split long turns, number them."""
    turns: list[list] = []
    for u in utterances:
        text = " ".join(u.text.split())
        if not text:
            continue
        last = turns[-1] if turns else None
        if last and last[0] == u.speaker and u.start - last[2] < 2.5 and len(last[3]) + len(text) < MAX_LINE_CHARS:
            last[2] = u.end
            last[3] += " " + text
        else:
            turns.append([u.speaker, u.start, u.end, text])

    lines: list[Line] = []
    for speaker, start, end, text in turns:
        pieces = _split(text)
        span = max(end - start, 0.0)
        total = sum(len(p) for p in pieces) or 1
        t = start
        for piece in pieces:
            dur = span * len(piece) / total
            lines.append(Line(len(lines) + 1, round(t, 2), round(t + dur, 2), speaker, piece))
            t += dur
    return lines


def _split(text: str) -> list[str]:
    if len(text) <= MAX_LINE_CHARS:
        return [text]
    out, cur = [], ""
    for sentence in _SENTENCE.split(text):
        if cur and len(cur) + len(sentence) > MAX_LINE_CHARS:
            out.append(cur)
            cur = sentence
        else:
            cur = f"{cur} {sentence}".strip()
    if cur:
        out.append(cur)
    return out


def render_lines(lines: list[Line]) -> str:
    return "\n".join(line.render() for line in lines)


# ---------------------------------------------------------------------------
# The minutes
# ---------------------------------------------------------------------------


@dataclass
class Chapter:
    title: str
    summary: str
    start: float = 0.0
    refs: list[int] = field(default_factory=list)


@dataclass
class Action:
    task: str
    owner: str = ""
    due: str = ""
    start: float = 0.0
    refs: list[int] = field(default_factory=list)
    done: bool = False


@dataclass
class Point:
    """A decision or an open question."""
    text: str
    start: float = 0.0
    refs: list[int] = field(default_factory=list)


@dataclass
class SpeakerStat:
    name: str
    seconds: float
    share: float  # 0..1 of all talk time
    turns: int


@dataclass
class Minutes:
    title: str = ""
    overview: str = ""
    highlights: list[str] = field(default_factory=list)
    chapters: list[Chapter] = field(default_factory=list)
    actions: list[Action] = field(default_factory=list)
    decisions: list[Point] = field(default_factory=list)
    questions: list[Point] = field(default_factory=list)
    speakers: list[SpeakerStat] = field(default_factory=list)
    language: str = "id"
    #: Notulen format version, so old meetings can be told apart and upgraded.
    version: int = 2

    def to_dict(self) -> dict:
        return asdict(self)

    @classmethod
    def from_dict(cls, data: dict) -> Minutes:
        def items(key, kind):
            return [kind(**{k: v for k, v in i.items() if k in kind.__dataclass_fields__})
                    for i in data.get(key) or [] if isinstance(i, dict)]
        return cls(
            title=str(data.get("title") or ""),
            overview=str(data.get("overview") or ""),
            highlights=[str(h) for h in data.get("highlights") or [] if str(h).strip()],
            chapters=items("chapters", Chapter),
            actions=items("actions", Action),
            decisions=items("decisions", Point),
            questions=items("questions", Point),
            speakers=items("speakers", SpeakerStat),
            language=data.get("language") if data.get("language") in ("id", "en") else "id",
            version=int(data.get("version") or 2),
        )


def is_rich(data: dict | None) -> bool:
    """True for the v2 shape, including upgraded v1 notulen saved back (version 1, v2 keys)."""
    return bool(data) and (int(data.get("version") or 1) >= 2 or "overview" in data or "chapters" in data)


# ---------------------------------------------------------------------------
# Grounding: turn model references into verified anchors
# ---------------------------------------------------------------------------

_REF = re.compile(r"L?(\d+)")


def parse_refs(raw, lines: list[Line]) -> list[int]:
    """Accept [12, "L13", "L14-L16", "15"]; keep only lines that exist, in order, no repeats."""
    if raw is None:
        return []
    if not isinstance(raw, list):
        raw = [raw]
    valid = {line.n for line in lines}
    out: list[int] = []
    for item in raw:
        nums = [int(x) for x in _REF.findall(str(item))]
        if len(nums) == 2 and "-" in str(item) and 0 < nums[1] - nums[0] <= 40:
            nums = list(range(nums[0], nums[1] + 1))
        for n in nums:
            if n in valid and n not in out:
                out.append(n)
    return sorted(out)


def anchor(refs: list[int], lines: list[Line]) -> float:
    by_n = {line.n: line for line in lines}
    return by_n[refs[0]].start if refs else 0.0


def speaker_of(refs: list[int], lines: list[Line]) -> str:
    by_n = {line.n: line for line in lines}
    names = Counter(by_n[n].speaker for n in refs if n in by_n)
    return names.most_common(1)[0][0] if names else ""


def speaker_stats(lines: list[Line]) -> list[SpeakerStat]:
    seconds: Counter = Counter()
    turns: Counter = Counter()
    previous = None
    for line in lines:
        seconds[line.speaker] += max(line.end - line.start, 0.0)
        if line.speaker != previous:
            turns[line.speaker] += 1
            previous = line.speaker
    total = sum(seconds.values()) or 1.0
    return [SpeakerStat(name, round(sec, 1), round(sec / total, 3), turns[name])
            for name, sec in seconds.most_common()]


# ---------------------------------------------------------------------------
# Owner matching
# ---------------------------------------------------------------------------

def match_owner(owner: str, speakers: list[str]) -> str:
    """Map a model-written owner to a real speaker name when it clearly refers to one.

    "Raka" -> "Raka Aditya Pratama", "Bima" -> "Bima Satria Wijaya".
    Generic labels from two-track recordings ("Saya", "Peserta") never match a
    first name by accident. Unknown names are kept as written (a person
    mentioned but not speaking can still own a task).
    """
    owner = " ".join(str(owner or "").split()).strip(" .:-")
    if not owner:
        return ""
    low = owner.lower()
    for s in speakers:
        if s.lower() == low:
            return s
    tokens = set(re.findall(r"\w+", low))
    hits = [s for s in speakers if tokens & set(re.findall(r"\w+", s.lower()))
            and s.lower() not in ("saya", "peserta", "pembicara", "unidentified speaker")]
    return hits[0] if len(hits) == 1 else owner


# ---------------------------------------------------------------------------
# Deduplication across chunks
# ---------------------------------------------------------------------------

def _words(text: str) -> set[str]:
    return {w for w in re.findall(r"\w+", text.lower()) if len(w) > 2}


def similar(a: str, b: str) -> float:
    wa, wb = _words(a), _words(b)
    if not wa or not wb:
        return 0.0
    return len(wa & wb) / min(len(wa), len(wb))


def dedupe_actions(actions: list[Action], threshold: float = 0.7) -> list[Action]:
    kept: list[Action] = []
    for a in sorted(actions, key=lambda x: x.start):
        twin = next((k for k in kept if (k.owner == a.owner or not k.owner or not a.owner)
                     and similar(k.task, a.task) >= threshold), None)
        if twin:
            twin.refs = sorted(set(twin.refs) | set(a.refs))
            twin.owner = twin.owner or a.owner
            twin.due = twin.due or a.due
            if len(a.task) > len(twin.task):
                twin.task = a.task
        else:
            kept.append(a)
    return kept


def dedupe_points(points: list[Point], threshold: float = 0.75) -> list[Point]:
    kept: list[Point] = []
    for p in sorted(points, key=lambda x: x.start):
        twin = next((k for k in kept if similar(k.text, p.text) >= threshold), None)
        if twin:
            twin.refs = sorted(set(twin.refs) | set(p.refs))
        else:
            kept.append(p)
    return kept


# ---------------------------------------------------------------------------
# Upgrading notulen written before version 2
# ---------------------------------------------------------------------------

def from_legacy(data: dict) -> Minutes:
    """Show a v1 notulen (summary/topics/decisions/action_items) in the v2 shape.

    Old meetings keep their content; they simply have no anchors until the
    notulen is regenerated.
    """
    actions = [Action(str(a.get("task") or ""), str(a.get("owner") or ""), str(a.get("due") or ""))
               for a in data.get("action_items") or [] if isinstance(a, dict) and a.get("task")]
    return Minutes(
        title=str(data.get("title") or ""),
        overview=str(data.get("summary") or ""),
        highlights=[str(t) for t in data.get("topics") or [] if str(t).strip()],
        actions=actions,
        decisions=[Point(str(d)) for d in data.get("decisions") or [] if str(d).strip()],
        questions=[Point(str(q)) for q in data.get("open_questions") or [] if str(q).strip()],
        language=data.get("language") if data.get("language") in ("id", "en") else "id",
        version=1,
    )


def load_minutes(data: dict | None) -> Minutes | None:
    if not data:
        return None
    return Minutes.from_dict(data) if is_rich(data) else from_legacy(data)
