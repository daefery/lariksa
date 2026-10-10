//! Meetings, settings and the glossary in one SQLite file.
//!
//! The transcript and notulen are stored as JSON on the meeting row; an FTS5
//! index over title, transcript and notulen powers search. One file next to the
//! app is the whole user library, which is what makes the folder portable.
//!
//! The schema and JSON shapes are exactly those of the Python version, so an
//! existing ``data/lariksa.db`` opens unchanged.

use anyhow::Result;
use chrono::{Datelike, Local, NaiveDateTime, Timelike};
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde_json::{json, Map, Value};
use std::path::Path;
use std::sync::Mutex;

use crate::hotkey::DEFAULT_HOTKEY;
use crate::minutes::{load_minutes, Minutes};
use crate::render::to_markdown;
use crate::text::random_hex;
use crate::transcript::{as_text, Utterance};

// Meeting lifecycle. "recording" is set while audio is captured; the worker
// moves a meeting through "queued" -> "transcribing" -> "summarizing" -> "done".
pub const RECORDING: &str = "recording";
pub const QUEUED: &str = "queued";
pub const TRANSCRIBING: &str = "transcribing";
pub const SUMMARIZING: &str = "summarizing";
pub const DONE: &str = "done";
pub const FAILED: &str = "failed";

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meetings (
    id          TEXT PRIMARY KEY,
    title       TEXT NOT NULL DEFAULT '',
    created_at  TEXT NOT NULL,
    duration    REAL NOT NULL DEFAULT 0,
    source      TEXT NOT NULL DEFAULT 'record',
    language    TEXT NOT NULL DEFAULT 'id',
    status      TEXT NOT NULL,
    error       TEXT NOT NULL DEFAULT '',
    transcript  TEXT NOT NULL DEFAULT '[]',
    notulen     TEXT NOT NULL DEFAULT '',
    timings     TEXT NOT NULL DEFAULT '{}'
);
CREATE VIRTUAL TABLE IF NOT EXISTS meetings_fts USING fts5(
    id UNINDEXED, title, body, tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TABLE IF NOT EXISTS settings (key TEXT PRIMARY KEY, value TEXT NOT NULL);
";

/// Settings and their defaults; only these keys are stored or returned.
pub fn default_settings() -> Map<String, Value> {
    let v = json!({
        "language": "id",            // what is spoken: id | en | "" (auto-detect); drives transcription
        "notulen_language": "auto",  // what the notulen is written in: auto | id | en
        "whisper_model": "auto",     // auto | whisper-turbo | whisper-small
        "glossary": [],
        "theme": "light",
        "hotkey": DEFAULT_HOTKEY,    // global start/stop shortcut; "" disables it
        "show_pill": true,           // floating always-on-top recording indicator
        "ui_language": "id",         // language of the app's own screens and messages: id | en
    });
    v.as_object().unwrap().clone()
}

pub const MONTHS_ID: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "Mei", "Jun", "Jul", "Agu", "Sep", "Okt", "Nov", "Des"];

/// "9 Okt 2026, 14:30", independent of the OS locale.
pub fn human_date(t: &NaiveDateTime) -> String {
    format!("{} {} {}, {:02}:{:02}", t.day(), MONTHS_ID[t.month0() as usize], t.year(), t.hour(), t.minute())
}

/// Title given to a meeting the user did not name; notulen may replace it.
pub fn default_title(english: bool, t: &NaiveDateTime) -> String {
    if english {
        format!("Meeting {}", human_date(t))
    } else {
        format!("Rapat {}", human_date(t))
    }
}

pub fn parse_iso(iso: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(iso, "%Y-%m-%dT%H:%M:%S").ok()
}

/// The human date of a stored timestamp, or the raw text if it does not parse.
pub fn human_date_of(iso: &str) -> String {
    parse_iso(iso).map_or_else(|| iso.to_string(), |t| human_date(&t))
}

#[derive(Debug, Clone)]
pub struct Meeting {
    pub id: String,
    pub title: String,
    pub created_at: String,
    pub status: String,
    pub duration: f64,
    pub source: String,
    pub language: String,
    pub error: String,
    pub transcript: Vec<Utterance>,
    pub notulen: Option<Minutes>,
    pub timings: Map<String, Value>,
}

impl Meeting {
    pub fn has_default_title(&self) -> bool {
        parse_iso(&self.created_at).is_some_and(|t| [false, true].iter().any(|en| self.title == default_title(*en, &t)))
    }

    pub fn summary_value(&self) -> Value {
        let preview: String = self.notulen.as_ref().map(|n| n.overview.chars().take(220).collect()).unwrap_or_default();
        json!({
            "id": self.id,
            "title": self.title,
            "created_at": self.created_at,
            "duration": self.duration,
            "status": self.status,
            "source": self.source,
            "preview": preview,
            "action_count": self.notulen.as_ref().map_or(0, |n| n.actions.len()),
        })
    }

    pub fn to_value(&self) -> Value {
        let mut v = self.summary_value();
        let o = v.as_object_mut().unwrap();
        o.insert("language".into(), json!(self.language));
        o.insert("error".into(), json!(self.error));
        o.insert("transcript".into(), serde_json::to_value(&self.transcript).unwrap());
        o.insert("notulen".into(), self.notulen.as_ref().map_or(Value::Null, Minutes::to_value));
        o.insert("timings".into(), Value::Object(self.timings.clone()));
        v
    }
}

fn row_to_meeting(row: &Row) -> rusqlite::Result<Meeting> {
    let transcript: String = row.get("transcript")?;
    let notulen: String = row.get("notulen")?;
    let timings: String = row.get("timings")?;
    let notulen = if notulen.is_empty() {
        None
    } else {
        serde_json::from_str::<Value>(&notulen).ok().and_then(|v| load_minutes(&v))
    };
    Ok(Meeting {
        id: row.get("id")?,
        title: row.get("title")?,
        created_at: row.get("created_at")?,
        status: row.get("status")?,
        duration: row.get("duration")?,
        source: row.get("source")?,
        language: row.get("language")?,
        error: row.get("error")?,
        transcript: serde_json::from_str(&transcript).unwrap_or_default(),
        notulen,
        timings: serde_json::from_str(if timings.is_empty() { "{}" } else { &timings }).unwrap_or_default(),
    })
}

/// Each word becomes a quoted prefix term, so user input can't break FTS syntax.
fn fts_query(text: &str) -> String {
    text.split_whitespace()
        .map(|t| t.replace('"', ""))
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\"*"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Fields that may change on a meeting after it is created.
#[derive(Default)]
pub struct Update {
    pub title: Option<String>,
    pub duration: Option<f64>,
    pub status: Option<String>,
    pub error: Option<String>,
    pub language: Option<String>,
    pub transcript: Option<Vec<Utterance>>,
    /// Some(None) clears the notulen.
    pub notulen: Option<Option<Minutes>>,
    pub timings: Option<Map<String, Value>>,
}

impl Update {
    pub fn status(s: &str) -> Self {
        Update { status: Some(s.into()), ..Default::default() }
    }
}

pub struct Store {
    conn: Mutex<Connection>,
}

impl Store {
    pub fn open(path: &Path) -> Result<Store> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Store { conn: Mutex::new(conn) })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // -- meetings --------------------------------------------------------

    pub fn create(&self, title: &str, source: &str, language: &str, status: &str, now: Option<NaiveDateTime>) -> Result<Meeting> {
        let now = now.unwrap_or_else(|| Local::now().naive_local()).with_nanosecond(0).unwrap();
        let meeting = Meeting {
            id: format!("{}-{}", now.format("%Y%m%d-%H%M%S"), random_hex(6)),
            title: if title.is_empty() { default_title(crate::i18n::is_english(), &now) } else { title.to_string() },
            created_at: now.format("%Y-%m-%dT%H:%M:%S").to_string(),
            status: status.into(),
            duration: 0.0,
            source: source.into(),
            language: language.into(),
            error: String::new(),
            transcript: vec![],
            notulen: None,
            timings: Map::new(),
        };
        self.conn().execute(
            "INSERT INTO meetings (id, title, created_at, source, language, status) VALUES (?, ?, ?, ?, ?, ?)",
            params![meeting.id, meeting.title, meeting.created_at, source, language, status],
        )?;
        self.reindex(&meeting.id)?;
        Ok(meeting)
    }

    pub fn get(&self, id: &str) -> Option<Meeting> {
        self.conn()
            .query_row("SELECT * FROM meetings WHERE id = ?", [id], row_to_meeting)
            .optional()
            .ok()
            .flatten()
    }

    pub fn list(&self, query: &str) -> Vec<Meeting> {
        let conn = self.conn();
        let fts = fts_query(query);
        let result = if !query.trim().is_empty() && !fts.is_empty() {
            conn.prepare(
                "SELECT m.* FROM meetings_fts f JOIN meetings m ON m.id = f.id \
                 WHERE meetings_fts MATCH ? ORDER BY m.created_at DESC",
            )
            .and_then(|mut s| s.query_map([fts], row_to_meeting)?.collect::<rusqlite::Result<Vec<_>>>())
        } else {
            conn.prepare("SELECT * FROM meetings ORDER BY created_at DESC")
                .and_then(|mut s| s.query_map([], row_to_meeting)?.collect::<rusqlite::Result<Vec<_>>>())
        };
        result.unwrap_or_default()
    }

    pub fn update(&self, id: &str, u: Update) -> Result<()> {
        let mut sets: Vec<(&str, rusqlite::types::Value)> = Vec::new();
        let reindex = u.title.is_some() || u.transcript.is_some() || u.notulen.is_some();
        if let Some(v) = u.title {
            sets.push(("title", v.into()));
        }
        if let Some(v) = u.duration {
            sets.push(("duration", v.into()));
        }
        if let Some(v) = u.status {
            sets.push(("status", v.into()));
        }
        if let Some(v) = u.error {
            sets.push(("error", v.into()));
        }
        if let Some(v) = u.language {
            sets.push(("language", v.into()));
        }
        if let Some(v) = u.transcript {
            sets.push(("transcript", serde_json::to_string(&v)?.into()));
        }
        if let Some(v) = u.notulen {
            sets.push(("notulen", v.map(|m| m.to_value().to_string()).unwrap_or_default().into()));
        }
        if let Some(v) = u.timings {
            sets.push(("timings", Value::Object(v).to_string().into()));
        }
        if sets.is_empty() {
            return Ok(());
        }
        let assignments = sets.iter().map(|(k, _)| format!("{k} = ?")).collect::<Vec<_>>().join(", ");
        let mut values: Vec<rusqlite::types::Value> = sets.into_iter().map(|(_, v)| v).collect();
        values.push(id.to_string().into());
        self.conn().execute(&format!("UPDATE meetings SET {assignments} WHERE id = ?"), rusqlite::params_from_iter(values))?;
        if reindex {
            self.reindex(id)?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn set_read_only_for_test(&self) {
        self.conn().pragma_update(None, "query_only", true).unwrap();
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM meetings WHERE id = ?", [id])?;
        tx.execute("DELETE FROM meetings_fts WHERE id = ?", [id])?;
        tx.commit()?;
        Ok(())
    }

    fn reindex(&self, id: &str) -> Result<()> {
        let Some(m) = self.get(id) else { return Ok(()) };
        let mut body = as_text(&m.transcript);
        if let Some(n) = &m.notulen {
            body = format!("{}\n{}", to_markdown(n, "", ""), body);
        }
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM meetings_fts WHERE id = ?", [id])?;
        tx.execute("INSERT INTO meetings_fts (id, title, body) VALUES (?, ?, ?)", params![id, m.title, body])?;
        tx.commit()?;
        Ok(())
    }

    fn ids(&self, sql: &str, args: &[&str]) -> Vec<String> {
        let conn = self.conn();
        let result = conn
            .prepare(sql)
            .and_then(|mut s| s.query_map(rusqlite::params_from_iter(args), |r| r.get(0))?.collect::<rusqlite::Result<Vec<String>>>());
        result.unwrap_or_default()
    }

    /// Meetings to resume after a restart, oldest first.
    pub fn pending(&self) -> Vec<String> {
        self.ids("SELECT id FROM meetings WHERE status IN (?, ?, ?) ORDER BY created_at", &[QUEUED, TRANSCRIBING, SUMMARIZING])
    }

    pub fn interrupted_recordings(&self) -> Vec<String> {
        self.ids("SELECT id FROM meetings WHERE status = ?", &[RECORDING])
    }

    // -- settings --------------------------------------------------------

    pub fn settings(&self) -> Map<String, Value> {
        let mut out = default_settings();
        let conn = self.conn();
        let rows = conn.prepare("SELECT key, value FROM settings").and_then(|mut s| {
            s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()
        });
        for (key, value) in rows.unwrap_or_default() {
            if out.contains_key(&key) {
                if let Ok(v) = serde_json::from_str(&value) {
                    out.insert(key, v);
                }
            }
        }
        out
    }

    pub fn save_settings(&self, values: &Map<String, Value>) -> Result<Map<String, Value>> {
        let defaults = default_settings();
        {
            let mut conn = self.conn();
            let tx = conn.transaction()?;
            for (key, value) in values {
                if !defaults.contains_key(key) {
                    continue;
                }
                let value = if key == "glossary" { json!(normalise_glossary(value)) } else { value.clone() };
                tx.execute(
                    "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                    params![key, value.to_string()],
                )?;
            }
            tx.commit()?;
        }
        Ok(self.settings())
    }
}

pub fn setting_str(settings: &Map<String, Value>, key: &str) -> String {
    match settings.get(key) {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

pub fn glossary_of(settings: &Map<String, Value>) -> Vec<String> {
    normalise_glossary(settings.get("glossary").unwrap_or(&Value::Null))
}

/// Accept a list or comma/newline text; trim, dedupe case-insensitively, keep order.
pub fn normalise_glossary(terms: &Value) -> Vec<String> {
    let raw: Vec<String> = match terms {
        Value::String(s) => s.replace('\n', ",").split(',').map(String::from).collect(),
        Value::Array(a) => a.iter().map(|v| crate::text::val_str(Some(v))).collect(),
        _ => vec![],
    };
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for term in raw {
        let term = term.trim().to_string();
        if !term.is_empty() && seen.insert(term.to_lowercase()) {
            out.push(term);
        }
    }
    out.truncate(200);
    out
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::minutes::Point;
    use crate::transcript::ME;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let s = Store::open(&dir.path().join("t.db")).unwrap();
        (dir, s)
    }

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> NaiveDateTime {
        chrono::NaiveDate::from_ymd_opt(y, mo, d).unwrap().and_hms_opt(h, mi, 0).unwrap()
    }

    #[test]
    fn create_get_update_roundtrip() {
        let _language = crate::i18n::test_lock();
        let (_d, store) = store();
        let m = store.create("", "record", "id", RECORDING, Some(at(2026, 10, 9, 14, 30))).unwrap();
        assert_eq!(m.title, "Rapat 9 Okt 2026, 14:30");
        assert!(m.has_default_title());
        assert_eq!(m.status, RECORDING);
        let mut timings = Map::new();
        timings.insert("asr".into(), json!(1.5));
        store
            .update(&m.id, Update {
                status: Some(DONE.into()),
                transcript: Some(vec![Utterance::new(ME, 0.0, 1.0, "Halo")]),
                notulen: Some(Some(Minutes { title: "Kickoff".into(), overview: "Mulai proyek".into(), ..Default::default() })),
                timings: Some(timings.clone()),
                ..Default::default()
            })
            .unwrap();
        let got = store.get(&m.id).unwrap();
        assert_eq!(got.status, DONE);
        assert_eq!(got.transcript[0].text, "Halo");
        assert_eq!(got.notulen.unwrap().title, "Kickoff");
        assert_eq!(got.timings, timings);
    }

    #[test]
    fn search_finds_transcript_and_notulen_words() {
        let (_d, store) = store();
        let a = store.create("Sprint review", "record", "id", RECORDING, None).unwrap();
        let b = store.create("Budget", "record", "id", RECORDING, None).unwrap();
        store.update(&a.id, Update { transcript: Some(vec![Utterance::new(ME, 0.0, 1.0, "Deployment ke staging besok")]), ..Default::default() }).unwrap();
        let n = Minutes { overview: "Anggaran pemasaran disetujui".into(), ..Default::default() };
        store.update(&b.id, Update { notulen: Some(Some(n)), ..Default::default() }).unwrap();
        assert_eq!(store.list("staging").iter().map(|m| m.id.clone()).collect::<Vec<_>>(), [a.id]);
        assert_eq!(store.list("anggar").iter().map(|m| m.id.clone()).collect::<Vec<_>>(), [b.id]);
        assert_eq!(store.list("").len(), 2);
    }

    #[test]
    fn search_input_cannot_break_fts() {
        let (_d, store) = store();
        store.create("x", "record", "id", RECORDING, None).unwrap();
        assert_eq!(store.list("\"").len(), store.list("").len());
        assert!(store.list("AND OR (").is_empty());
    }

    #[test]
    fn pending_and_interrupted() {
        let (_d, store) = store();
        let a = store.create("", "record", "id", QUEUED, None).unwrap();
        let b = store.create("", "record", "id", RECORDING, None).unwrap();
        assert_eq!(store.pending(), [a.id]);
        assert_eq!(store.interrupted_recordings(), [b.id]);
    }

    #[test]
    fn settings_defaults_and_glossary_normalised() {
        let (_d, store) = store();
        assert_eq!(store.settings()["language"], "id");
        let saved = store
            .save_settings(json!({"glossary": "Budi, PIC,\n budi , Solve Education", "evil": 1}).as_object().unwrap())
            .unwrap();
        assert_eq!(saved["glossary"], json!(["Budi", "PIC", "Solve Education"]));
        assert!(!saved.contains_key("evil"));
    }

    #[test]
    fn normalise_glossary_caps_length() {
        let terms: Vec<Value> = (0..500).map(|i| json!(format!("t{i}"))).collect();
        assert_eq!(normalise_glossary(&Value::Array(terms)).len(), 200);
    }

    #[test]
    fn decisions_are_searchable() {
        let (_d, store) = store();
        let a = store.create("x", "record", "id", RECORDING, None).unwrap();
        let n = Minutes { decisions: vec![Point::new("Platform dipisah", 0.0, vec![])], ..Default::default() };
        store.update(&a.id, Update { notulen: Some(Some(n)), ..Default::default() }).unwrap();
        assert_eq!(store.list("dipisah").len(), 1);
    }
}
