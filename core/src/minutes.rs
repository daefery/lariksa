//! Rich meeting minutes: chapters, action items and decisions anchored to the transcript.
//!
//! Everything the model writes must point back to evidence. Each chapter, action
//! item, decision and open question carries the transcript lines it came from
//! (``refs``); the start time, the speaker label and the "jump to" link are
//! derived from those lines here, in code, never typed by the model. A claim with
//! no valid reference is dropped, so nothing ungrounded reaches the page.
//!
//! The model sees the transcript as numbered lines (``L12 [04:23] Nadia: ...``)
//! and answers with line numbers. That is what lets a 4B local model produce
//! Read.ai-grade structure: it never has to copy timestamps or names, it only
//! has to read and point.

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
#[cfg(test)]
use serde_json::json;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::LazyLock;

use crate::text::{char_len, py_round, squash, truthy, val_str};
use crate::transcript::{clock, Utterance};

// ---------------------------------------------------------------------------
// Line view of a transcript
// ---------------------------------------------------------------------------

/// Turns longer than this are split at sentence ends so a reference can point
/// to the exact moment, not the start of a five-minute monologue.
pub const MAX_LINE_CHARS: usize = 420;

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub n: usize,
    pub start: f64,
    pub end: f64,
    pub speaker: String,
    pub text: String,
}

impl Line {
    pub fn new(n: usize, start: f64, end: f64, speaker: &str, text: &str) -> Self {
        Self { n, start, end, speaker: speaker.into(), text: text.into() }
    }

    pub fn render(&self) -> String {
        format!("L{} [{}] {}: {}", self.n, clock(self.start), self.speaker, self.text)
    }
}

/// Merge same-speaker utterances into turns, split long turns, number them.
pub fn lines_from(utterances: &[Utterance]) -> Vec<Line> {
    struct Turn {
        speaker: String,
        start: f64,
        end: f64,
        text: String,
    }
    let mut turns: Vec<Turn> = Vec::new();
    for u in utterances {
        let text = squash(&u.text);
        if text.is_empty() {
            continue;
        }
        match turns.last_mut() {
            Some(last)
                if last.speaker == u.speaker
                    && u.start - last.end < 2.5
                    && char_len(&last.text) + char_len(&text) < MAX_LINE_CHARS =>
            {
                last.end = u.end;
                last.text.push(' ');
                last.text.push_str(&text);
            }
            _ => turns.push(Turn { speaker: u.speaker.clone(), start: u.start, end: u.end, text }),
        }
    }

    let mut lines: Vec<Line> = Vec::new();
    for turn in turns {
        let pieces = split_sentences(&turn.text);
        let span = (turn.end - turn.start).max(0.0);
        let total = pieces.iter().map(|p| char_len(p)).sum::<usize>().max(1) as f64;
        let mut t = turn.start;
        for piece in pieces {
            let dur = span * char_len(&piece) as f64 / total;
            lines.push(Line {
                n: lines.len() + 1,
                start: py_round(t, 2),
                end: py_round(t + dur, 2),
                speaker: turn.speaker.clone(),
                text: piece,
            });
            t += dur;
        }
    }
    lines
}

/// Python ``re.split(r"(?<=[.!?…])\s+", text)``: split on whitespace that follows sentence punctuation.
fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        cur.push(c);
        if matches!(c, '.' | '!' | '?' | '…') && chars.peek().is_some_and(|n| n.is_whitespace()) {
            while chars.peek().is_some_and(|n| n.is_whitespace()) {
                chars.next();
            }
            out.push(std::mem::take(&mut cur));
        }
    }
    out.push(cur);
    out
}

fn split_sentences(text: &str) -> Vec<String> {
    if char_len(text) <= MAX_LINE_CHARS {
        return vec![text.to_string()];
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    for sentence in sentences(text) {
        if !cur.is_empty() && char_len(&cur) + char_len(&sentence) > MAX_LINE_CHARS {
            out.push(std::mem::replace(&mut cur, sentence));
        } else {
            cur = format!("{cur} {sentence}").trim().to_string();
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

pub fn render_lines(lines: &[Line]) -> String {
    lines.iter().map(Line::render).collect::<Vec<_>>().join("\n")
}

// ---------------------------------------------------------------------------
// The minutes
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Chapter {
    pub title: String,
    pub summary: String,
    #[serde(default)]
    pub start: f64,
    #[serde(default)]
    pub refs: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub task: String,
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub due: String,
    #[serde(default)]
    pub start: f64,
    #[serde(default)]
    pub refs: Vec<usize>,
    #[serde(default)]
    pub done: bool,
    /// The model's quote, used by the review pass only; never stored.
    #[serde(skip)]
    pub quote: String,
}

/// Value equality on the stored fields only (the review quote is scratch data).
impl PartialEq for Action {
    fn eq(&self, o: &Self) -> bool {
        self.task == o.task
            && self.owner == o.owner
            && self.due == o.due
            && self.start == o.start
            && self.refs == o.refs
            && self.done == o.done
    }
}

impl Action {
    pub fn new(task: &str, owner: &str, due: &str, start: f64, refs: Vec<usize>) -> Self {
        Self { task: task.into(), owner: owner.into(), due: due.into(), start, refs, done: false, quote: String::new() }
    }
}

/// A decision or an open question.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub text: String,
    #[serde(default)]
    pub start: f64,
    #[serde(default)]
    pub refs: Vec<usize>,
}

impl Point {
    pub fn new(text: &str, start: f64, refs: Vec<usize>) -> Self {
        Self { text: text.into(), start, refs }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpeakerStat {
    pub name: String,
    pub seconds: f64,
    /// 0..1 of all talk time
    pub share: f64,
    pub turns: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Minutes {
    pub title: String,
    pub overview: String,
    pub highlights: Vec<String>,
    pub chapters: Vec<Chapter>,
    pub actions: Vec<Action>,
    pub decisions: Vec<Point>,
    pub questions: Vec<Point>,
    pub speakers: Vec<SpeakerStat>,
    pub language: String,
    /// Notulen format version, so old meetings can be told apart and upgraded.
    pub version: i64,
}

impl Default for Minutes {
    fn default() -> Self {
        Self {
            title: String::new(),
            overview: String::new(),
            highlights: vec![],
            chapters: vec![],
            actions: vec![],
            decisions: vec![],
            questions: vec![],
            speakers: vec![],
            language: "id".into(),
            version: 2,
        }
    }
}

fn str_field(data: &Value, key: &str) -> String {
    val_str(data.get(key))
}

fn list<'a>(data: &'a Value, key: &str) -> Vec<&'a Value> {
    match data.get(key) {
        Some(Value::Array(items)) => items.iter().collect(),
        _ => vec![],
    }
}

fn num(v: Option<&Value>) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(0.0)
}

fn refs_of(v: Option<&Value>) -> Vec<usize> {
    match v {
        Some(Value::Array(items)) => items.iter().filter_map(|x| x.as_u64().map(|n| n as usize)).collect(),
        _ => vec![],
    }
}

fn language_of(data: &Value) -> String {
    match data.get("language").and_then(Value::as_str) {
        Some(l @ ("id" | "en")) => l.into(),
        _ => "id".into(),
    }
}

impl Minutes {
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).expect("minutes serialise")
    }

    /// Tolerant reader for stored notulen: unknown keys ignored, missing ones defaulted.
    pub fn from_value(data: &Value) -> Minutes {
        let chapters = list(data, "chapters")
            .into_iter()
            .filter(|i| i.is_object())
            .map(|c| Chapter {
                title: str_field(c, "title"),
                summary: str_field(c, "summary"),
                start: num(c.get("start")),
                refs: refs_of(c.get("refs")),
            })
            .collect();
        let actions = list(data, "actions")
            .into_iter()
            .filter(|i| i.is_object())
            .map(|a| Action {
                task: str_field(a, "task"),
                owner: str_field(a, "owner"),
                due: str_field(a, "due"),
                start: num(a.get("start")),
                refs: refs_of(a.get("refs")),
                done: a.get("done").is_some_and(truthy),
                quote: String::new(),
            })
            .collect();
        let points = |key| {
            list(data, key)
                .into_iter()
                .filter(|i| i.is_object())
                .map(|p| Point { text: str_field(p, "text"), start: num(p.get("start")), refs: refs_of(p.get("refs")) })
                .collect()
        };
        let speakers = list(data, "speakers")
            .into_iter()
            .filter(|i| i.is_object())
            .map(|s| SpeakerStat {
                name: str_field(s, "name"),
                seconds: num(s.get("seconds")),
                share: num(s.get("share")),
                turns: s.get("turns").and_then(Value::as_u64).unwrap_or(0) as usize,
            })
            .collect();
        Minutes {
            title: str_field(data, "title"),
            overview: str_field(data, "overview"),
            highlights: list(data, "highlights")
                .into_iter()
                .map(|h| val_str(Some(h)))
                .filter(|h| !h.trim().is_empty())
                .collect(),
            chapters,
            actions,
            decisions: points("decisions"),
            questions: points("questions"),
            speakers,
            language: language_of(data),
            version: data.get("version").and_then(Value::as_i64).filter(|v| *v != 0).unwrap_or(2),
        }
    }
}

/// True for the v2 shape, including upgraded v1 notulen saved back (version 1, v2 keys).
pub fn is_rich(data: &Value) -> bool {
    let Some(obj) = data.as_object() else { return false };
    if obj.is_empty() {
        return false;
    }
    let version = data.get("version").and_then(Value::as_i64).filter(|v| *v != 0).unwrap_or(1);
    version >= 2 || obj.contains_key("overview") || obj.contains_key("chapters")
}

// ---------------------------------------------------------------------------
// Grounding: turn model references into verified anchors
// ---------------------------------------------------------------------------

static REF: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"L?(\d+)").unwrap());

/// Accept [12, "L13", "L14-L16", "15"]; keep only lines that exist, in order, no repeats.
pub fn parse_refs(raw: Option<&Value>, lines: &[Line]) -> Vec<usize> {
    let items: Vec<&Value> = match raw {
        None | Some(Value::Null) => return vec![],
        Some(Value::Array(a)) => a.iter().collect(),
        Some(other) => vec![other],
    };
    let valid: HashSet<usize> = lines.iter().map(|l| l.n).collect();
    let mut out: Vec<usize> = Vec::new();
    for item in items {
        let text = match item {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        let mut nums: Vec<usize> = REF.captures_iter(&text).filter_map(|c| c[1].parse().ok()).collect();
        if nums.len() == 2 && text.contains('-') && nums[1] > nums[0] && nums[1] - nums[0] <= 40 {
            nums = (nums[0]..=nums[1]).collect();
        }
        for n in nums {
            if valid.contains(&n) && !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out.sort_unstable();
    out
}

pub fn anchor(refs: &[usize], lines: &[Line]) -> f64 {
    refs.first().and_then(|n| lines.iter().find(|l| l.n == *n)).map_or(0.0, |l| l.start)
}

/// Talk time per speaker, most first (ties keep first-seen order, like ``Counter.most_common``).
pub fn speaker_stats(lines: &[Line]) -> Vec<SpeakerStat> {
    let mut order: Vec<String> = Vec::new();
    let mut seconds: HashMap<String, f64> = HashMap::new();
    let mut turns: HashMap<String, usize> = HashMap::new();
    let mut previous: Option<&str> = None;
    for line in lines {
        if !seconds.contains_key(&line.speaker) {
            order.push(line.speaker.clone());
        }
        *seconds.entry(line.speaker.clone()).or_default() += (line.end - line.start).max(0.0);
        if previous != Some(line.speaker.as_str()) {
            *turns.entry(line.speaker.clone()).or_default() += 1;
            previous = Some(&line.speaker);
        }
    }
    let total: f64 = seconds.values().sum();
    let total = if total == 0.0 { 1.0 } else { total };
    order.sort_by(|a, b| seconds[b].total_cmp(&seconds[a]));
    order
        .into_iter()
        .map(|name| SpeakerStat {
            seconds: py_round(seconds[&name], 1),
            share: py_round(seconds[&name] / total, 3),
            turns: turns.get(&name).copied().unwrap_or(0),
            name,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Owner matching
// ---------------------------------------------------------------------------

static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\w+").unwrap());

fn words(text: &str) -> HashSet<String> {
    WORD.find_iter(&text.to_lowercase()).map(|m| m.as_str().to_string()).collect()
}

/// Map a model-written owner to a real speaker name when it clearly refers to one.
///
/// "Raka" -> "Raka Aditya Pratama". Generic labels from two-track recordings
/// ("Saya", "Peserta") never match a first name by accident. Unknown names are
/// kept as written (a person mentioned but not speaking can still own a task).
pub fn match_owner(owner: &str, speakers: &[String]) -> String {
    let owner = squash(owner);
    let owner = owner.trim_matches(|c| matches!(c, ' ' | '.' | ':' | '-')).to_string();
    if owner.is_empty() {
        return String::new();
    }
    let low = owner.to_lowercase();
    if let Some(s) = speakers.iter().find(|s| s.to_lowercase() == low) {
        return s.clone();
    }
    let tokens = words(&low);
    let hits: Vec<&String> = speakers
        .iter()
        .filter(|s| {
            !tokens.is_disjoint(&words(s))
                && !matches!(s.to_lowercase().as_str(), "saya" | "peserta" | "pembicara" | "unidentified speaker")
        })
        .collect();
    if hits.len() == 1 { hits[0].clone() } else { owner }
}

// ---------------------------------------------------------------------------
// Deduplication across chunks
// ---------------------------------------------------------------------------

fn long_words(text: &str) -> HashSet<String> {
    words(text).into_iter().filter(|w| w.chars().count() > 2).collect()
}

pub fn similar(a: &str, b: &str) -> f64 {
    let (wa, wb) = (long_words(a), long_words(b));
    if wa.is_empty() || wb.is_empty() {
        return 0.0;
    }
    wa.intersection(&wb).count() as f64 / wa.len().min(wb.len()) as f64
}

fn union(a: &[usize], b: &[usize]) -> Vec<usize> {
    a.iter().chain(b).copied().collect::<BTreeSet<_>>().into_iter().collect()
}

pub fn dedupe_actions(mut actions: Vec<Action>, threshold: f64) -> Vec<Action> {
    actions.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut kept: Vec<Action> = Vec::new();
    for a in actions {
        let twin = kept.iter_mut().find(|k| {
            (k.owner == a.owner || k.owner.is_empty() || a.owner.is_empty()) && similar(&k.task, &a.task) >= threshold
        });
        match twin {
            Some(twin) => {
                twin.refs = union(&twin.refs, &a.refs);
                if twin.owner.is_empty() {
                    twin.owner = a.owner.clone();
                }
                if twin.due.is_empty() {
                    twin.due = a.due.clone();
                }
                if char_len(&a.task) > char_len(&twin.task) {
                    twin.task = a.task.clone();
                }
            }
            None => kept.push(a),
        }
    }
    kept
}

pub fn dedupe_points(mut points: Vec<Point>, threshold: f64) -> Vec<Point> {
    points.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut kept: Vec<Point> = Vec::new();
    for p in points {
        match kept.iter_mut().find(|k| similar(&k.text, &p.text) >= threshold) {
            Some(twin) => twin.refs = union(&twin.refs, &p.refs),
            None => kept.push(p),
        }
    }
    kept
}

// ---------------------------------------------------------------------------
// Upgrading notulen written before version 2
// ---------------------------------------------------------------------------

/// Show a v1 notulen (summary/topics/decisions/action_items) in the v2 shape.
///
/// Old meetings keep their content; they simply have no anchors until the
/// notulen is regenerated.
pub fn from_legacy(data: &Value) -> Minutes {
    let actions = list(data, "action_items")
        .into_iter()
        .filter(|a| a.is_object() && a.get("task").is_some_and(truthy))
        .map(|a| Action::new(&str_field(a, "task"), &str_field(a, "owner"), &str_field(a, "due"), 0.0, vec![]))
        .collect();
    let strings = |key| -> Vec<String> {
        list(data, key).into_iter().map(|v| val_str(Some(v))).filter(|s| !s.trim().is_empty()).collect()
    };
    Minutes {
        title: str_field(data, "title"),
        overview: str_field(data, "summary"),
        highlights: strings("topics"),
        actions,
        decisions: strings("decisions").iter().map(|d| Point::new(d, 0.0, vec![])).collect(),
        questions: strings("open_questions").iter().map(|q| Point::new(q, 0.0, vec![])).collect(),
        language: language_of(data),
        version: 1,
        ..Minutes::default()
    }
}

pub fn load_minutes(data: &Value) -> Option<Minutes> {
    if !truthy(data) {
        return None;
    }
    Some(if is_rich(data) { Minutes::from_value(data) } else { from_legacy(data) })
}


#[cfg(test)]
mod tests {
    use super::*;

    fn speakers() -> Vec<String> {
        ["Raka Aditya Pratama", "Nadia Kusuma", "Bima Satria Wijaya", "Unidentified Speaker"]
            .map(String::from)
            .to_vec()
    }

    #[test]
    fn lines_merge_same_speaker_and_number_them() {
        let lines = lines_from(&[
            Utterance::new("Nadia", 0.0, 2.0, "Oke kita mulai."),
            Utterance::new("Nadia", 2.5, 4.0, "Agenda pertama rilis."),
            Utterance::new("Raka", 5.0, 7.0, "Siap."),
        ]);
        let got: Vec<_> = lines.iter().map(|l| (l.n, l.speaker.as_str(), l.text.as_str())).collect();
        assert_eq!(got, [(1, "Nadia", "Oke kita mulai. Agenda pertama rilis."), (2, "Raka", "Siap.")]);
        assert_eq!(render_lines(&lines).lines().nth(1).unwrap(), "L2 [00:05] Raka: Siap.");
    }

    #[test]
    fn long_monologue_splits_at_sentences_with_increasing_times() {
        let text = (0..30)
            .map(|i| format!("Kalimat nomor {i} cukup panjang untuk menguji pemotongan baris."))
            .collect::<Vec<_>>()
            .join(" ");
        let lines = lines_from(&[Utterance::new("Nadia", 100.0, 400.0, &text)]);
        assert!(lines.len() > 3);
        assert!(lines.iter().all(|l| char_len(&l.text) <= 420));
        assert!(lines.windows(2).all(|w| w[0].start <= w[1].start));
        assert!(lines[0].start == 100.0 && lines.last().unwrap().end <= 400.01);
        assert_eq!(lines.iter().map(|l| l.text.as_str()).collect::<Vec<_>>().join(" "), text);
    }

    #[test]
    fn parse_refs_accepts_messy_model_output_and_drops_invented_lines() {
        let lines: Vec<Line> = (1..=20).map(|n| Line::new(n, n as f64 * 10.0, n as f64 * 10.0 + 5.0, "A", "x")).collect();
        assert_eq!(parse_refs(Some(&json!(["L3", 5, "L7-L9", "L99", "foo", "L3"])), &lines), [3, 5, 7, 8, 9]);
        assert_eq!(parse_refs(Some(&json!("L12")), &lines), [12]);
        assert_eq!(parse_refs(None, &lines), Vec::<usize>::new());
        assert_eq!(parse_refs(Some(&json!(["L1-L900"])), &lines), [1]); // absurd ranges are not expanded
    }

    #[test]
    fn match_owner_to_speakers() {
        let s = speakers();
        assert_eq!(match_owner("Raka", &s), "Raka Aditya Pratama");
        assert_eq!(match_owner("bima", &s), "Bima Satria Wijaya");
        assert_eq!(match_owner("Nadia Kusuma", &s), "Nadia Kusuma");
        assert_eq!(match_owner("Melati", &s), "Melati");
        assert_eq!(match_owner("  ", &s), "");
        let two = ["Saya".to_string(), "Peserta".to_string()];
        assert_eq!(match_owner("Saya", &two), "Saya");
        assert_eq!(match_owner("Budi", &two), "Budi");
    }

    #[test]
    fn speaker_stats_share_and_turns() {
        let lines = [Line::new(1, 0.0, 60.0, "A", "x"), Line::new(2, 60.0, 80.0, "B", "y"), Line::new(3, 80.0, 100.0, "A", "z")];
        let stats = speaker_stats(&lines);
        let got: Vec<_> = stats.iter().map(|s| (s.name.as_str(), s.seconds, s.turns)).collect();
        assert_eq!(got, [("A", 80.0, 2), ("B", 20.0, 1)]);
        assert!((stats.iter().map(|s| s.share).sum::<f64>() - 1.0).abs() < 0.01);
    }

    #[test]
    fn dedupe_actions_merges_same_task_from_two_chunks() {
        let a = Action::new("Deploy Edu.Platform ke production tanggal 16", "Raka Aditya Pratama", "16 Okt", 1000.0, vec![40]);
        let b = Action::new("Deploy Edu.Platform ke production pada tanggal 16", "Raka Aditya Pratama", "", 1100.0, vec![44]);
        let c = Action::new("Siapkan strategi re-engagement 180.000 pengguna", "Tri Samsul", "", 2500.0, vec![90]);
        let out = dedupe_actions(vec![a, b, c], 0.7);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].refs, [40, 44]);
        assert_eq!(out[0].due, "16 Okt");
    }

    #[test]
    fn dedupe_points_merges() {
        let out = dedupe_points(
            vec![Point::new("Prototype Community dikunci", 10.0, vec![5]), Point::new("Prototype community siap dikunci", 20.0, vec![9])],
            0.75,
        );
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].refs, [5, 9]);
    }

    #[test]
    fn roundtrip_and_version() {
        let m = Minutes { title: "T".into(), actions: vec![Action::new("x", "", "", 0.0, vec![1])], language: "en".into(), ..Default::default() };
        let back = Minutes::from_value(&m.to_value());
        assert_eq!(back, m);
        assert!(is_rich(&m.to_value()));
        assert!(!is_rich(&json!({"title": "old", "summary": "v1 notulen"})));
        // An upgraded v1 notulen saved back keeps loading as v2 (not re-read as legacy and emptied).
        let up = from_legacy(&json!({"title": "old", "summary": "s", "action_items": [{"task": "Do", "owner": "A"}]}));
        assert_eq!(up.version, 1);
        assert_eq!(load_minutes(&up.to_value()).unwrap().actions[0].task, "Do");
    }

    #[test]
    fn python_written_notulen_loads() {
        // Exactly what the Python app stored (dataclasses.asdict + json.dumps).
        let stored = json!({"title": "Rapat rilis", "overview": "Ok", "highlights": ["Rilis Jumat"],
            "chapters": [{"title": "Rilis", "summary": "Tim membahas rilis.", "start": 0.0, "refs": [1]}],
            "actions": [{"task": "Deploy", "owner": "Budi", "due": "", "start": 0.0, "refs": [1], "done": false}],
            "decisions": [{"text": "Rilis hari Jumat", "start": 0.0, "refs": [1]}], "questions": [],
            "speakers": [{"name": "Saya", "seconds": 1.0, "share": 1.0, "turns": 1}], "language": "id", "version": 2});
        let m = load_minutes(&stored).unwrap();
        assert_eq!(m.to_value(), stored);
    }
}
