//! Which language the notulen should be written in.
//!
//! The notulen follows what was actually said, not the transcription setting:
//! Whisper writes English speech as English even when told to expect Indonesian,
//! so the transcript text is the reliable signal.
//!
//! Detection counts function words, which carry the grammar of a sentence.
//! "Jadi kita need to align soal deadline-nya" is Indonesian with English nouns,
//! and its function words (jadi, kita, soal, nya) are Indonesian.

use regex::Regex;
use std::collections::HashSet;
use std::sync::LazyLock;

pub const SUPPORTED: [&str; 2] = ["id", "en"];
pub const AUTO: &str = "auto";

static ID: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    "yang dan di ini itu untuk dengan tidak ada kita saya ke dari akan ya gitu nggak gak enggak \
     aja saja sudah udah bisa juga karena kalau kalo jadi tapi atau sama lagi mau harus belum \
     kami kamu mereka dia apa bagaimana gimana kenapa sih kan dong nya terus soal biar supaya \
     nanti sekarang masih lebih sangat banyak semua tolong baik"
        .split_whitespace()
        .collect()
});
static EN: LazyLock<HashSet<&str>> = LazyLock::new(|| {
    "the and to of is are we you that it for in on this be will let's with have so was what how \
     can do not but they our just i it's there would should could about from been were has \
     please need let us"
        .split_whitespace()
        .collect()
});
static WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[a-z']+").unwrap());

/// "en" or "id" by function-word count; ``default`` when there is no signal.
///
/// Pass spoken text only, without speaker labels ("Saya:" would count as
/// Indonesian on every line).
pub fn detect(text: &str, default: &str) -> String {
    let lower = text.to_lowercase();
    let (mut indonesian, mut english) = (0, 0);
    for w in WORD.find_iter(&lower) {
        indonesian += ID.contains(w.as_str()) as usize;
        english += EN.contains(w.as_str()) as usize;
    }
    if english > indonesian {
        "en".into()
    } else if indonesian > english {
        "id".into()
    } else if SUPPORTED.contains(&default) {
        default.into()
    } else {
        "id".into()
    }
}

/// The notulen language: a fixed choice, or detected from the transcript.
pub fn resolve(setting: &str, spoken_text: &str, fallback: &str) -> String {
    if SUPPORTED.contains(&setting) {
        return setting.into();
    }
    detect(spoken_text, fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_meeting() {
        assert_eq!(detect("Good morning everyone. We will have the meeting today about testing the application.", "id"), "en");
        assert_eq!(detect("Another test for application testing.", "id"), "en");
    }

    #[test]
    fn indonesian_meeting() {
        assert_eq!(detect("Halo semuanya, saya lagi mau mencoba rekaman ini.", "id"), "id");
        assert_eq!(detect("Jadi kita need to align soal deadline-nya, biar release minggu ini aman.", "id"), "id");
    }

    #[test]
    fn no_signal_uses_default() {
        assert_eq!(detect("", "en"), "en");
        assert_eq!(detect("OKR Q4", "id"), "id");
        assert_eq!(detect("", ""), "id");
    }

    #[test]
    fn resolve_fixed_setting_wins() {
        let english = "We will review the plan and the budget.";
        assert_eq!(resolve("id", english, "id"), "id");
        assert_eq!(resolve("en", "Kita bahas anggarannya ya.", "id"), "en");
        assert_eq!(resolve("auto", english, "id"), "en");
        assert_eq!(resolve("", english, "id"), "en");
    }
}
