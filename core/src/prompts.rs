//! Prompts, JSON schemas and UI labels, shared word for word with the Python
//! version the notulen quality was tuned on (``assets/prompts.json``).

use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

#[derive(Deserialize)]
pub struct Writer {
    pub system: String,
    pub extract: String,
    pub actions: String,
    pub review: String,
    pub overview: String,
}

#[derive(Deserialize)]
struct Bundle {
    writer: HashMap<String, Writer>,
    labels: HashMap<String, HashMap<String, String>>,
    asr: HashMap<String, String>,
    schemas: HashMap<String, Value>,
}

static BUNDLE: LazyLock<Bundle> =
    LazyLock::new(|| serde_json::from_str(include_str!("../assets/prompts.json")).expect("prompts.json is valid"));

/// The four model calls of the minutes writer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Schema {
    Topics,
    Actions,
    Review,
    Overview,
}

impl Schema {
    pub fn json(self) -> &'static Value {
        let key = match self {
            Schema::Topics => "topics",
            Schema::Actions => "actions",
            Schema::Review => "review",
            Schema::Overview => "overview",
        };
        &BUNDLE.schemas[key]
    }
}

pub fn writer(language: &str) -> &'static Writer {
    BUNDLE.writer.get(language).unwrap_or(&BUNDLE.writer["id"])
}

/// Every user-visible heading, per language, shared by screen, Markdown, WhatsApp and DOCX.
pub fn labels(language: &str) -> &'static HashMap<String, String> {
    BUNDLE.labels.get(language).unwrap_or(&BUNDLE.labels["id"])
}

pub fn label(language: &str, key: &str) -> &'static str {
    labels(language).get(key).map(String::as_str).unwrap_or("")
}

/// Primes Whisper toward the register of a work meeting with English loanwords.
pub fn asr_base(language: &str) -> &'static str {
    BUNDLE.asr.get(language).unwrap_or(&BUNDLE.asr["id"])
}

/// ``str.format`` for the few named fields the prompts use.
pub fn fill(template: &str, fields: &[(&str, &str)]) -> String {
    let mut out = template.to_string();
    for (key, value) in fields {
        out = out.replace(&format!("{{{key}}}"), value);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundle_loads_both_languages() {
        assert!(writer("id").system.contains("Bahasa Indonesia"));
        assert!(writer("en").system.contains("careful meeting note-taker"));
        assert_eq!(writer("fr").system, writer("id").system);
        assert_eq!(label("en", "summary"), "Summary");
        assert_eq!(label("id", "owner"), "PIC");
        assert!(Schema::Actions.json()["properties"]["actions"]["maxItems"] == 10);
    }
}
