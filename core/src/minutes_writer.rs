//! Write rich minutes with a small local model.
//!
//! Two passes, both small enough for a 4B model with an 8k context:
//!
//! 1. **Read** each chunk of numbered transcript lines (~12k characters) and
//!    extract chapters, action items, decisions and open questions, each with the
//!    line numbers it came from. Output is capped per chunk so the JSON always closes.
//! 2. **Write** the overview, highlights and title from the chapter summaries
//!    (not the raw transcript), which is short and fits any meeting length.
//!
//! Merging, deduplication, owners, timestamps and speaker stats happen in code.

use regex::Regex;
use serde_json::{json, Value};
use std::sync::LazyLock;

use crate::minutes::{
    anchor, dedupe_actions, dedupe_points, match_owner, parse_refs, render_lines, speaker_stats, Action, Chapter, Line,
    Minutes, Point,
};
use crate::prompts::{fill, writer, Schema, Writer};
use crate::text::{char_len, squash, take_chars, val_str};
use crate::transcript::clock;

/// One schema-constrained model answer: (system, user, schema) -> JSON.
pub type Complete<'a> = dyn FnMut(&str, &str, Schema) -> anyhow::Result<Value> + 'a;

/// Characters of numbered transcript per extraction call (~4k tokens of Indonesian).
pub const CHUNK_CHARS: usize = 12_000;

/// Words a small model writes instead of leaving owner/due blank. Prompts ask for
/// an empty string, but it still answers "empty", "nanti", "TBD"... so they are
/// cleared here, whatever the language.
const PLACEHOLDERS: &[&str] = &[
    "", "-", "--", "?", "empty", "none", "null", "n/a", "na", "tbd", "tba", "unknown", "later", "soon", "asap",
    "not specified", "unspecified", "not mentioned", "nanti", "segera", "tidak ada", "belum ada", "tidak disebut",
    "belum ditentukan", "kosong",
];

pub fn clean_placeholder(value: Option<&Value>) -> String {
    let text = val_str(value).trim().to_string();
    let key = text.to_lowercase();
    let key = key.trim_matches(|c| matches!(c, ' ' | '.' | '(' | ')'));
    if PLACEHOLDERS.contains(&key) { String::new() } else { text }
}

/// Split on line boundaries; never cut a line.
pub fn chunk_lines(lines: &[Line], size: usize) -> Vec<Vec<Line>> {
    let mut out: Vec<Vec<Line>> = Vec::new();
    let mut cur: Vec<Line> = Vec::new();
    let mut length = 0;
    for line in lines {
        let rendered = char_len(&line.render()) + 1;
        if !cur.is_empty() && length + rendered > size {
            out.push(std::mem::take(&mut cur));
            length = 0;
        }
        cur.push(line.clone());
        length += rendered;
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

// --- Guardrails ----------------------------------------------------------------
// A 4B model leaks prompt artefacts and pads lists. These checks run on every
// answer, so the page only shows what the transcript supports.

static REF_TEXT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*[\[(]?\bL\d+(?:\s*[-–,]\s*L?\d+)*[\])]?").unwrap());
static CLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(?:pada\s+|at\s+)?(?:hari|menit|minute|pukul)?\s*\b\d{1,2}:\d{2}(?::\d{2})?\b").unwrap()
});
static NUM: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+").unwrap());
static SPACE_BEFORE_PUNCT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+([.,;:])").unwrap());
static DATE_PHRASE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)\b(?:tanggal|tgl\.?)\s*\d{1,2}(?:\s+(?:jan\w*|feb\w*|mar\w*|apr\w*|mei|jun\w*|jul\w*|agu\w*|sep\w*|",
        r"okt\w*|nov\w*|des\w*))?|\b\d{1,2}\s+(?:jan\w*|feb\w*|mar\w*|apr\w*|mei|may|jun\w*|jul\w*|agu\w*|aug\w*|",
        r"sep\w*|okt\w*|oct\w*|nov\w*|des\w*|dec\w*)|\bend of (?:the )?\w+|\bminggu depan\b|\bnext week\b|\bbesok\b|",
        r"\btomorrow\b|\b(?:senin|selasa|rabu|kamis|jumat|sabtu|minggu|monday|tuesday|wednesday|thursday|friday)\b",
        r"|\b(?:dalam\s+)?(?:satu|1)\s+hari\b|\bakhir tahun\b|\bend of (?:the )?year\b"
    ))
    .unwrap()
});
/// Things said during the meeting, not work to do after it.
static IN_MEETING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"(?i)^(?:menjelaskan|melaporkan|menanyakan|mempertanyakan|mengklarifikasi|menyampaikan|mempresentasikan|",
        r"meminta klarifikasi|memberikan apresiasi|menegaskan|menekankan|explain\w*|report(?:ed)?\b|ask(?:ed)? about|",
        r"clarif\w*|present(?:ed)?\b|emphasi[sz]\w*)"
    ))
    .unwrap()
});
static QUESTION_CUE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)\?|\b(?:apakah|bagaimana|gimana|kenapa|mengapa|kapan|siapa|belum|nanti dibahas|how|what|why|when|who|not yet)\b")
        .unwrap()
});

/// Remove leaked line refs and transcript clock times; tidy spacing.
pub fn clean_text(text: &str) -> String {
    let text = REF_TEXT.replace_all(text, "");
    let text = CLOCK.replace_all(&text, "");
    let text = SPACE_BEFORE_PUNCT.replace_all(&squash(&text), "$1").to_string();
    text.trim_matches(|c| matches!(c, ' ' | ',' | ';')).to_string()
}

fn clean_value(v: Option<&Value>) -> String {
    clean_text(&val_str(v))
}

/// Keep a due date only if its numbers were actually said; else recover the phrase from the source.
pub fn ground_due(due: &str, source: &str) -> String {
    let due = clean_text(due);
    let due_nums: Vec<&str> = NUM.find_iter(&due).map(|m| m.as_str()).collect();
    let source_nums: Vec<&str> = NUM.find_iter(source).map(|m| m.as_str()).collect();
    let spoken_numbers = !due.is_empty() && !due_nums.is_empty() && due_nums.iter().all(|n| source_nums.contains(n));
    let spoken_words =
        !due.is_empty() && due_nums.is_empty() && source.to_lowercase().contains(&due.to_lowercase());
    if spoken_numbers || spoken_words {
        return due;
    }
    match DATE_PHRASE.find(source) {
        Some(found) if !due.is_empty() => found.as_str().to_string(),
        _ => String::new(),
    }
}

fn text_of(refs: &[usize], lines: &[Line]) -> String {
    refs.iter()
        .filter_map(|n| lines.iter().find(|l| l.n == *n))
        .map(|l| l.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

fn items<'a>(raw: &'a Value, key: &str) -> Vec<&'a Value> {
    match raw.get(key) {
        Some(Value::Array(a)) => a.iter().collect(),
        _ => vec![],
    }
}

#[derive(Default)]
pub struct Grounded {
    pub chapters: Vec<Chapter>,
    pub actions: Vec<Action>,
    pub decisions: Vec<Point>,
    pub questions: Vec<Point>,
}

/// Turn one model answer into anchored items; drop anything without valid refs in this chunk.
pub fn ground(raw: &Value, chunk: &[Line], all: &[Line], speakers: &[String]) -> Grounded {
    let mut g = Grounded::default();
    for c in items(raw, "chapters") {
        let refs = parse_refs(c.get("refs"), chunk);
        let (title, summary) = (clean_value(c.get("title")), clean_value(c.get("summary")));
        if !refs.is_empty() && !title.is_empty() && !summary.is_empty() {
            g.chapters.push(Chapter { title, summary, start: anchor(&refs, all), refs });
        }
    }
    for a in items(raw, "actions") {
        let refs = parse_refs(a.get("refs"), chunk);
        let task = clean_value(a.get("task"));
        let kind_ok = match a.get("kind") {
            None | Some(Value::Null) => true,
            Some(k) => matches!(k.as_str(), Some("will_do" | "asked_to_do" | "follow_up")),
        };
        if refs.is_empty() || task.is_empty() || !kind_ok || IN_MEETING.is_match(&task) {
            continue;
        }
        let source = text_of(&refs, all);
        let mut action = Action::new(
            &task,
            &match_owner(&clean_placeholder(a.get("owner")), speakers),
            &ground_due(&clean_placeholder(a.get("due")), &source),
            anchor(&refs, all),
            refs,
        );
        let quote = clean_value(a.get("quote"));
        action.quote = if quote.is_empty() { take_chars(&source, 200) } else { quote };
        g.actions.push(action);
    }
    for (key, is_question) in [("decisions", false), ("questions", true)] {
        for p in items(raw, key) {
            let refs = parse_refs(p.get("refs"), chunk);
            let text = clean_value(p.get("text"));
            if refs.is_empty() || text.is_empty() {
                continue;
            }
            // A question must have been asked: its source lines carry a question cue.
            if is_question && !QUESTION_CUE.is_match(&text_of(&refs, all)) {
                continue;
            }
            let point = Point::new(&text, anchor(&refs, all), refs);
            if is_question { g.questions.push(point) } else { g.decisions.push(point) }
        }
    }
    g
}

fn sorted_by_start(mut actions: Vec<Action>) -> Vec<Action> {
    actions.sort_by(|a, b| a.start.total_cmp(&b.start));
    actions
}

/// Second look at every candidate at once: drop in-meeting chatter, sharpen task wording.
///
/// The review may only choose among existing candidates (by id) and rewrite the
/// wording; references, times and due dates stay as grounded in the first pass.
pub fn review_actions(actions: Vec<Action>, complete: &mut Complete, prompts: &Writer, speakers: &[String]) -> Vec<Action> {
    if actions.is_empty() {
        return vec![];
    }
    let listing = actions
        .iter()
        .enumerate()
        .map(|(i, a)| {
            format!(
                "{}. owner={} | task={} | due={} | kutipan=\"{}\"",
                i + 1,
                if a.owner.is_empty() { "-" } else { &a.owner },
                a.task,
                if a.due.is_empty() { "-" } else { &a.due },
                a.quote
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let answer = match complete(&prompts.system, &fill(&prompts.review, &[("items", &listing)]), Schema::Review) {
        Ok(answer) => answer,
        Err(err) => {
            log::warn!("Action review skipped ({err}); keeping grounded candidates");
            return sorted_by_start(actions);
        }
    };
    let rows = match (answer.get("items"), answer.get("keep")) {
        (Some(Value::Array(rows)), _) if !rows.is_empty() => rows.clone(),
        (_, Some(Value::Array(rows))) => rows.clone(),
        _ => vec![],
    };
    let mut chosen: Vec<usize> = Vec::new();
    let mut actions = actions;
    for row in rows {
        let verdict = row.get("verdict").and_then(Value::as_str).unwrap_or("follow_up");
        if verdict != "follow_up" {
            continue;
        }
        let index = match row.get("id") {
            Some(Value::Number(n)) => n.as_f64().map(|f| f as i64),
            Some(Value::String(s)) => s.trim().parse::<i64>().ok(),
            _ => None,
        };
        let Some(index) = index.map(|i| i - 1) else { continue };
        if index < 0 || index as usize >= actions.len() || chosen.contains(&(index as usize)) {
            continue;
        }
        let index = index as usize;
        let action = &mut actions[index];
        let task = clean_value(row.get("task"));
        // Keep the review's wording only when it stays short; padded rewrites lose to the original.
        if !task.is_empty() && !IN_MEETING.is_match(&task) && task.split_whitespace().count() <= 18 {
            action.task = task;
        }
        let owner = match_owner(&clean_placeholder(row.get("owner")), speakers);
        if !owner.is_empty() {
            action.owner = owner;
        }
        chosen.push(index);
    }
    let kept: Vec<Action> = chosen.into_iter().map(|i| actions[i].clone()).collect();
    sorted_by_start(kept)
}

/// Chapters that start within ``gap`` seconds of each other are one topic split in two.
pub fn merge_close_chapters(chapters: Vec<Chapter>, gap: f64) -> Vec<Chapter> {
    let mut out: Vec<Chapter> = Vec::new();
    for c in chapters {
        match out.last_mut() {
            Some(prev) if c.start - prev.start < gap => {
                prev.refs = prev.refs.iter().chain(&c.refs).copied().collect::<std::collections::BTreeSet<_>>().into_iter().collect();
                if char_len(&c.summary) > char_len(&prev.summary) {
                    prev.title = c.title;
                    prev.summary = c.summary;
                }
            }
            _ => out.push(c),
        }
    }
    out
}

pub fn write_minutes(
    lines: &[Line],
    complete: &mut Complete,
    language: &str,
    on_progress: &mut dyn FnMut(f64),
) -> anyhow::Result<Minutes> {
    let p = writer(language);
    let speakers: Vec<String> = speaker_stats(lines).into_iter().map(|s| s.name).collect();
    let chunks = chunk_lines(lines, CHUNK_CHARS);
    let steps = (2 * chunks.len() + 2) as f64;
    let total = chunks.len().to_string();

    let mut all = Grounded::default();
    let mut failures = 0usize;

    for (i, chunk) in chunks.iter().enumerate() {
        let index = (i + 1).to_string();
        let text = render_lines(chunk);
        let fields = [("index", index.as_str()), ("total", total.as_str()), ("text", text.as_str())];

        // A failed chunk is skipped, not fatal, unless failures are systematic.
        let mut attempt = |user: String, schema: Schema, empty: Value| -> anyhow::Result<Value> {
            match complete(&p.system, &user, schema) {
                Ok(v) => Ok(v),
                Err(err) => {
                    failures += 1;
                    log::warn!("Minutes chunk skipped ({err})");
                    if failures >= chunks.len().max(2) {
                        return Err(err);
                    }
                    Ok(empty)
                }
            }
        };
        let topics = attempt(fill(&p.extract, &fields), Schema::Topics, json!({"chapters": [], "decisions": [], "questions": []}))?;
        on_progress((2 * i + 1) as f64 / steps);
        let todo = attempt(fill(&p.actions, &fields), Schema::Actions, json!({"actions": []}))?;

        let mut raw = topics;
        if !raw.is_object() {
            raw = json!({});
        }
        raw["actions"] = todo.get("actions").cloned().unwrap_or(Value::Null);
        let g = ground(&raw, chunk, lines, &speakers);
        all.chapters.extend(g.chapters);
        all.actions.extend(g.actions);
        all.decisions.extend(g.decisions);
        all.questions.extend(g.questions);
        on_progress((2 * i + 2) as f64 / steps);
    }

    all.chapters.sort_by(|a, b| a.start.total_cmp(&b.start));
    let chapters = merge_close_chapters(all.chapters, 45.0);
    let actions = review_actions(dedupe_actions(all.actions, 0.7), complete, p, &speakers);
    let decisions = dedupe_points(all.decisions, 0.75);
    let questions = dedupe_points(all.questions, 0.75);

    let head = if chapters.is_empty() {
        json!({})
    } else {
        let chapter_text = chapters
            .iter()
            .map(|c| format!("[{}] {}: {}", clock(c.start), c.title, c.summary))
            .collect::<Vec<_>>()
            .join("\n");
        let decision_text = if decisions.is_empty() {
            "-".to_string()
        } else {
            decisions.iter().map(|d| format!("- {}", d.text)).collect::<Vec<_>>().join("\n")
        };
        complete(&p.system, &fill(&p.overview, &[("chapters", &chapter_text), ("decisions", &decision_text)]), Schema::Overview)?
    };
    on_progress(1.0);

    let highlights = match head.get("highlights") {
        Some(Value::Array(h)) => h.iter().map(|x| clean_value(Some(x))).filter(|h| !h.is_empty()).take(8).collect(),
        _ => vec![],
    };
    Ok(Minutes {
        title: clean_value(head.get("title")),
        overview: clean_value(head.get("overview")),
        highlights,
        chapters,
        actions,
        decisions,
        questions,
        speakers: speaker_stats(lines),
        language: language.to_string(),
        version: 2,
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::minutes::lines_from;
    use crate::transcript::Utterance;
    use anyhow::anyhow;

    fn meeting(n: usize) -> Vec<Line> {
        let names = ["Nadia Kusuma", "Raka Aditya Pratama", "Bima Satria Wijaya"];
        let utts: Vec<Utterance> = (0..n)
            .map(|i| {
                Utterance::new(
                    names[i % 3],
                    (i * 30) as f64,
                    (i * 30 + 25) as f64,
                    &format!("Kalimat ke {i} tentang rilis tanggal 16 Okt dan migrasi data. ").repeat(4),
                )
            })
            .collect();
        lines_from(&utts)
    }

    /// Answers like a small model: messy refs, a placeholder due date, one invented line.
    #[derive(Default)]
    pub struct FakeModel {
        pub calls: Vec<(String, String, Schema)>,
    }

    impl FakeModel {
        pub fn answer(&mut self, system: &str, user: &str, schema: Schema) -> anyhow::Result<Value> {
            self.calls.push((system.into(), user.into(), schema));
            Ok(match schema {
                Schema::Review => {
                    let n = user.matches("owner=").count();
                    let mut items = vec![json!({"id": 1, "verdict": "follow_up", "owner": "Raka", "task": "Deploy Edu.Platform ke production"})];
                    items.extend((2..=n).map(|i| json!({"id": i, "verdict": "follow_up", "owner": "", "task": ""})));
                    items.push(json!({"id": 999, "verdict": "follow_up", "owner": "", "task": "x"}));
                    json!({ "items": items })
                }
                Schema::Overview => json!({"title": "Sinkronisasi rilis", "overview": "Rapat membahas rilis.",
                    "highlights": ["Rilis 16 Oktober", "", "Migrasi 91 persen"]}),
                Schema::Actions | Schema::Topics => {
                    let first: usize = user
                        .split("\nL")
                        .nth(1)
                        .and_then(|s| s.split(' ').next())
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(1);
                    if schema == Schema::Actions {
                        json!({"actions": [
                            {"quote": "aku deploy", "kind": "will_do", "owner": "Raka", "task": "Deploy ke production tanggal 16",
                             "due": "nanti", "refs": [format!("L{}", first + 1)]},
                            {"quote": "deploy tgl 16", "kind": "will_do", "owner": "Raka",
                             "task": "Deploy ke production pada tanggal 16", "due": "16 Okt", "refs": [format!("L{}", first + 2)]},
                            {"quote": "sudah fix", "kind": "already_done", "owner": "Raka", "task": "Fix edge case",
                             "due": "", "refs": [format!("L{first}")]}]})
                    } else {
                        json!({
                            "chapters": [{"title": "Rencana rilis", "summary": "Raka menjelaskan rilis.",
                                          "refs": [format!("L{first}"), format!("L{}", first + 1)]},
                                         {"title": "Topik tanpa bukti", "summary": "x", "refs": ["L99999"]}],
                            "decisions": [{"text": "Prototype dikunci", "refs": [format!("L{first}")]}],
                            "questions": [{"text": "Siapa yang testing?", "refs": []}]})
                    }
                }
            })
        }
    }

    #[test]
    fn chunks_cover_every_line_once() {
        let lines = meeting(60);
        let chunks = chunk_lines(&lines, 3000);
        assert!(chunks.len() > 2);
        let flat: Vec<usize> = chunks.iter().flatten().map(|l| l.n).collect();
        assert_eq!(flat, lines.iter().map(|l| l.n).collect::<Vec<_>>());
    }

    #[test]
    fn write_minutes_grounds_merges_and_anchors() {
        let lines = meeting(60);
        let mut model = FakeModel::default();
        let mut progress = vec![];
        let m = write_minutes(&lines, &mut |s, u, k| model.answer(s, u, k), "id", &mut |v| progress.push(v)).unwrap();

        let chunks = chunk_lines(&lines, CHUNK_CHARS).len();
        assert_eq!(model.calls.len(), 2 * chunks + 2);
        let kinds: Vec<Schema> = model.calls[..model.calls.len() - 2].iter().map(|c| c.2).collect();
        assert_eq!(kinds, [Schema::Topics, Schema::Actions].repeat(chunks));
        let review = &model.calls[model.calls.len() - 2];
        assert!(review.2 == Schema::Review && review.1.contains("kutipan="));
        assert_eq!(m.actions[0].task, "Deploy Edu.Platform ke production");
        assert_eq!(*progress.last().unwrap(), 1.0);

        assert_eq!(m.title, "Sinkronisasi rilis");
        assert_eq!(m.highlights, ["Rilis 16 Oktober", "Migrasi 91 persen"]);
        assert!(m.chapters.iter().all(|c| c.title != "Topik tanpa bukti"));
        assert_eq!(m.chapters[0].start, lines[0].start);
        assert!(m.chapters.windows(2).all(|w| w[0].start <= w[1].start));
        assert!(m.actions.iter().all(|a| a.owner == "Raka Aditya Pratama"));
        assert!(m.actions.iter().any(|a| a.due == "16 Okt"));
        assert!(!m.actions.iter().any(|a| a.due == "nanti"));
        assert!(!m.actions.iter().any(|a| a.task == "Fix edge case"));
        assert!(m.questions.is_empty());
        assert_eq!(m.decisions.len(), 1);
        assert_eq!(m.decisions[0].refs.len(), chunks);
        assert!(["Nadia Kusuma", "Raka Aditya Pratama", "Bima Satria Wijaya"].contains(&m.speakers[0].name.as_str()));
    }

    #[test]
    fn english_prompts_for_english_minutes() {
        let mut model = FakeModel::default();
        write_minutes(&meeting(6), &mut |s, u, k| model.answer(s, u, k), "en", &mut |_| {}).unwrap();
        let (system, user, _) = &model.calls[0];
        assert!(system.contains("careful meeting note-taker") && user.contains("Part 1 of"));
    }

    #[test]
    fn clean_text_strips_leaked_refs_and_clock_times() {
        assert_eq!(clean_text("Nama Play Class ditolak board [L6, L7, L8]."), "Nama Play Class ditolak board.");
        assert_eq!(clean_text("Nadia menyampaikan pada hari 04:23 bahwa platform dipisah"), "Nadia menyampaikan bahwa platform dipisah");
        assert_eq!(clean_text("Migrasi 91 persen, pipeline 84 persen"), "Migrasi 91 persen, pipeline 84 persen");
    }

    #[test]
    fn due_dates_must_be_spoken() {
        let said = "Terus setelah itu di tanggal 16, terus di deploy produktinya di tanggal 16";
        assert_eq!(ground_due("2024-10-16", said), "tanggal 16");
        assert_eq!(ground_due("04:45-05:43 (dalam rapat)", "aku lagi ngajek ke tas"), "");
        assert_eq!(ground_due("minggu depan", "kita udah mulai minggu depan"), "minggu depan");
        assert_eq!(ground_due("", "tanggal 30"), "");
    }

    #[test]
    fn in_meeting_statements_are_not_action_items() {
        let lines = [Line::new(1, 0.0, 10.0, "Yoga", "Jadi scoring automation itu masih hardcoded."),
                     Line::new(2, 10.0, 20.0, "Raka", "Aku deploy ke production tanggal 16 ya.")];
        let raw = json!({"chapters": [], "decisions": [], "questions": [], "actions": [
            {"kind": "will_do", "owner": "Yoga", "task": "Menjelaskan proses scoring otomatis", "due": "", "refs": ["L1"]},
            {"kind": "discussion", "owner": "Raka", "task": "Deploy ke production", "due": "", "refs": ["L2"]},
            {"kind": "will_do", "owner": "Raka", "task": "Deploy ke production", "due": "2025-10-16", "refs": ["L2"]}]});
        let g = ground(&raw, &lines, &lines, &["Yoga".into(), "Raka".into()]);
        let got: Vec<_> = g.actions.iter().map(|a| (a.owner.as_str(), a.task.as_str(), a.due.as_str())).collect();
        assert_eq!(got, [("Raka", "Deploy ke production", "tanggal 16")]);
    }

    #[test]
    fn invented_questions_are_dropped() {
        let lines = [Line::new(1, 0.0, 10.0, "Nadia", "Backend nyatu, sip."),
                     Line::new(2, 10.0, 20.0, "Velia", "Tanggal 30 bisa dipakai buat games nggak?")];
        let raw = json!({"chapters": [], "actions": [], "decisions": [], "questions": [
            {"text": "Apakah integrasi backend memengaruhi keamanan?", "refs": ["L1"]},
            {"text": "Bisakah platform dipakai untuk games tanggal 30?", "refs": ["L2"]}]});
        let g = ground(&raw, &lines, &lines, &[]);
        assert_eq!(g.questions.iter().map(|q| q.text.as_str()).collect::<Vec<_>>(), ["Bisakah platform dipakai untuk games tanggal 30?"]);
    }

    #[test]
    fn empty_meeting_skips_overview_call() {
        let mut model = FakeModel::default();
        let m = write_minutes(&[], &mut |s, u, k| model.answer(s, u, k), "id", &mut |_| {}).unwrap();
        assert!(model.calls.is_empty() && m.chapters.is_empty() && m.title.is_empty());
    }

    #[test]
    fn chapters_starting_close_together_merge() {
        let out = merge_close_chapters(vec![
            Chapter { title: "A".into(), summary: "short".into(), start: 156.0, refs: vec![6] },
            Chapter { title: "B".into(), summary: "a longer summary".into(), start: 182.0, refs: vec![8] },
            Chapter { title: "C".into(), summary: "x".into(), start: 400.0, refs: vec![20] },
        ], 45.0);
        let got: Vec<_> = out.iter().map(|c| (c.title.as_str(), c.start, c.refs.clone())).collect();
        assert_eq!(got, [("B", 156.0, vec![6, 8]), ("C", 400.0, vec![20])]);
    }

    #[test]
    fn review_can_only_choose_existing_candidates() {
        let cands = vec![Action::new("Run through prototype", "Dimas", "", 10.0, vec![3]),
                         Action::new("Deploy ke production", "Raka", "tanggal 16", 20.0, vec![5])];
        let mut model = |_: &str, _: &str, schema: Schema| -> anyhow::Result<Value> {
            assert_eq!(schema, Schema::Review);
            Ok(json!({"items": [{"id": 1, "verdict": "in_meeting", "owner": "Dimas", "task": "Run through prototype"},
                {"id": 2, "verdict": "follow_up", "owner": "Raka Aditya Pratama", "task": "Deploy Edu.Platform ke production"},
                {"id": 2, "verdict": "follow_up", "owner": "", "task": "dup"},
                {"id": 7, "verdict": "follow_up", "owner": "X", "task": "invented"},
                {"id": "abc", "verdict": "follow_up", "owner": "", "task": ""}]}))
        };
        let speakers = ["Raka Aditya Pratama".to_string(), "Dimas Arya Putra".to_string()];
        let out = review_actions(cands, &mut model, writer("id"), &speakers);
        let got: Vec<_> = out.iter().map(|a| (a.owner.as_str(), a.task.as_str(), a.due.as_str(), a.refs.clone())).collect();
        assert_eq!(got, [("Raka Aditya Pratama", "Deploy Edu.Platform ke production", "tanggal 16", vec![5])]);
        assert!(review_actions(vec![], &mut model, writer("id"), &[]).is_empty());
    }

    #[test]
    fn one_broken_chunk_is_skipped_not_fatal() {
        let lines = meeting(60);
        let mut model = FakeModel::default();
        let mut n = 0;
        let m = write_minutes(&lines, &mut |s, u, k| {
            n += 1;
            if k == Schema::Actions && n == 2 {
                return Err(anyhow!("Model mengembalikan JSON yang tidak valid."));
            }
            model.answer(s, u, k)
        }, "id", &mut |_| {}).unwrap();
        assert!(m.title == "Sinkronisasi rilis" && !m.actions.is_empty());

        let broken = write_minutes(&lines, &mut |_, _, _| Err(anyhow!("Model mengembalikan JSON yang tidak valid.")), "id", &mut |_| {});
        assert!(broken.is_err());
    }

    #[test]
    fn failed_review_keeps_grounded_candidates() {
        let cands = vec![Action::new("B", "", "", 20.0, vec![2]), Action::new("A", "", "", 10.0, vec![1])];
        let out = review_actions(cands, &mut |_, _, _| Err(anyhow!("invalid JSON")), writer("id"), &[]);
        assert_eq!(out.iter().map(|a| a.task.as_str()).collect::<Vec<_>>(), ["A", "B"]);
    }

    #[test]
    fn placeholder_owner_and_due_are_cleared() {
        for (raw, want) in [("empty", ""), ("Tidak ada", ""), ("N/A", ""), ("(TBD)", ""), ("Friday", "Friday"), ("nanti", "")] {
            assert_eq!(clean_placeholder(Some(&json!(raw))), want);
        }
    }
}
