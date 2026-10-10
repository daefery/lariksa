//! The meeting transcript: who said what, and when.
//!
//! A meeting is recorded as two tracks. The microphone is you ("Saya"); the
//! system loopback is everyone else ("Peserta"). Each track is transcribed on its
//! own and the two are merged by time, which gives reliable speaker labels with
//! no diarization model.
//!
//! The one trap is bleed: on laptop speakers the mic also hears the other
//! participants, so their words show up twice, once on each track. A mic utterance
//! that overlaps a system utterance in time and says nearly the same thing is
//! that echo, and is dropped.

use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;

use crate::text::sequence_ratio;

pub const ME: &str = "Saya";
pub const OTHERS: &str = "Peserta";

/// Similarity above which an overlapping mic utterance is treated as echo.
pub const BLEED_SIMILARITY: f64 = 0.6;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Utterance {
    pub speaker: String,
    pub start: f64,
    pub end: f64,
    pub text: String,
}

impl Utterance {
    pub fn new(speaker: &str, start: f64, end: f64, text: &str) -> Self {
        Self { speaker: speaker.into(), start, end, text: text.into() }
    }

    pub fn overlaps(&self, other: &Utterance) -> bool {
        self.start < other.end && other.start < self.end
    }
}

static NON_WORD: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[^\w\s]").unwrap());

fn normalise(text: &str) -> String {
    NON_WORD.replace_all(&text.to_lowercase(), "").trim().to_string()
}

fn similar(a: &str, b: &str) -> f64 {
    sequence_ratio(&normalise(a), &normalise(b))
}

fn is_echo(mine: &Utterance, theirs: &[Utterance]) -> bool {
    for other in theirs {
        if other.start >= mine.end {
            break;
        }
        if mine.overlaps(other) && similar(&mine.text, &other.text) >= BLEED_SIMILARITY {
            return true;
        }
    }
    false
}

fn by_time(a: &Utterance, b: &Utterance) -> std::cmp::Ordering {
    a.start.total_cmp(&b.start).then(a.end.total_cmp(&b.end))
}

/// Interleave both tracks by start time, dropping mic echo of the others.
pub fn merge_tracks(mine: Vec<Utterance>, mut theirs: Vec<Utterance>) -> Vec<Utterance> {
    theirs.sort_by(|a, b| a.start.total_cmp(&b.start));
    let mut out: Vec<Utterance> = mine.into_iter().filter(|u| !is_echo(u, &theirs)).collect();
    out.extend(theirs);
    out.sort_by(by_time); // stable, like Python's sorted
    out
}

/// "mm:ss", or "h:mm:ss" past an hour.
pub fn clock(seconds: f64) -> String {
    let total = seconds.max(0.0) as i64;
    let (hours, rest) = (total / 3600, total % 3600);
    let (minutes, secs) = (rest / 60, rest % 60);
    if hours > 0 {
        format!("{hours}:{minutes:02}:{secs:02}")
    } else {
        format!("{minutes:02}:{secs:02}")
    }
}

/// Plain text with speaker turns merged, the form search indexes.
pub fn as_text(utterances: &[Utterance]) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut speaker: Option<&str> = None;
    for u in utterances {
        if speaker == Some(u.speaker.as_str()) && !lines.is_empty() {
            let last = lines.last_mut().unwrap();
            last.push(' ');
            last.push_str(&u.text);
        } else {
            lines.push(format!("[{}] {}: {}", clock(u.start), u.speaker, u.text));
            speaker = Some(&u.speaker);
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str, a: f64, b: f64, t: &str) -> Utterance {
        Utterance::new(s, a, b, t)
    }

    #[test]
    fn merge_interleaves_by_time() {
        let mine = vec![u(ME, 0.0, 2.0, "Halo semua"), u(ME, 10.0, 12.0, "Oke, saya follow up")];
        let theirs = vec![u(OTHERS, 3.0, 6.0, "Pagi, kita mulai ya")];
        let merged = merge_tracks(mine, theirs);
        let texts: Vec<_> = merged.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, ["Halo semua", "Pagi, kita mulai ya", "Oke, saya follow up"]);
    }

    #[test]
    fn mic_echo_of_others_is_dropped() {
        let theirs = vec![u(OTHERS, 5.0, 9.0, "Deadline-nya hari Jumat ya, tolong dicek")];
        let mine = vec![u(ME, 5.3, 9.2, "deadline nya hari jumat ya tolong di cek")];
        assert_eq!(merge_tracks(mine, theirs.clone()), theirs);
    }

    #[test]
    fn similar_words_at_a_different_time_are_kept() {
        let theirs = vec![u(OTHERS, 5.0, 9.0, "Deadline-nya hari Jumat")];
        let mine = vec![u(ME, 20.0, 23.0, "Deadline-nya hari Jumat")];
        assert_eq!(merge_tracks(mine, theirs).len(), 2);
    }

    #[test]
    fn overlapping_but_different_speech_is_kept() {
        let theirs = vec![u(OTHERS, 5.0, 9.0, "Budget kita tinggal sedikit")];
        let mine = vec![u(ME, 6.0, 8.0, "Setuju, saya yang urus")];
        assert_eq!(merge_tracks(mine, theirs).len(), 2);
    }

    #[test]
    fn as_text_merges_consecutive_turns() {
        let text = as_text(&[u(ME, 0.0, 1.0, "Satu."), u(ME, 1.0, 2.0, "Dua."), u(OTHERS, 65.0, 66.0, "Tiga.")]);
        assert_eq!(text, "[00:00] Saya: Satu. Dua.\n[01:05] Peserta: Tiga.");
    }

    #[test]
    fn clock_shows_hours_only_when_needed() {
        assert_eq!(clock(59.0), "00:59");
        assert_eq!(clock(3725.0), "1:02:05");
    }
}
