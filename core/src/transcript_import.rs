//! Import an existing transcript (Read.ai, Google Meet, Zoom, Teams, plain text).
//!
//! Lariksa can then write its minutes for meetings it did not record. Supported
//! shapes, all common in exported transcripts:
//!
//! ```text
//! 0:00 - Raka Aditya Pratama            (Read.ai, Meet notes)
//! So first, a bit of context...
//!
//! [00:02:08] Nadia Kusuma: But also...   (one line per turn)
//!
//! 00:02:08.000 --> 00:02:10.000          (WebVTT / Zoom / Teams)
//! <v Nadia Kusuma>But also...</v>
//! ```
//!
//! Speaker turns end where the next one starts; the last turn gets a nominal
//! length so talk-time stats stay sane.

use regex::Regex;
use std::path::Path;
use std::sync::LazyLock;

use crate::text::squash;
use crate::transcript::Utterance;

pub const TEXT_TYPES: [&str; 4] = [".txt", ".vtt", ".srt", ".md"];
pub const LAST_TURN_SECONDS: f64 = 10.0;

const TIME: &str = r"(\d{1,2}:\d{2}(?::\d{2})?(?:[.,]\d{1,3})?)";
static HEADER: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?m)^[ \t]*\[?{TIME}\]?[ \t]*[-–—][ \t]*([^\n]+?)[ \t]*$")).unwrap());
static INLINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(&format!(r"(?m)^[ \t]*\[?{TIME}\]?[ \t]+([^:\n]{{1,60}}?):[ \t]*(\S[^\n]*)$")).unwrap());
static VTT_TIMES: LazyLock<Regex> = LazyLock::new(|| Regex::new(&format!(r"{TIME}\s*-->\s*{TIME}[^\n]*\n")).unwrap());
static VTT_VOICE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<v\s+([^>]+)>(.*?)(?:</v>|$)").unwrap());
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());
static BLANK_LINE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n[ \t\r\x0b\x0c]*\n").unwrap());

pub fn seconds(stamp: &str) -> f64 {
    let parts: Vec<f64> = stamp.replace(',', ".").split(':').filter_map(|p| p.parse().ok()).collect();
    match parts.as_slice() {
        [m, s] => m * 60.0 + s,
        [h, m, s] => h * 3600.0 + m * 60.0 + s,
        _ => 0.0,
    }
}

fn from_starts(turns: &[(f64, String, String)]) -> Vec<Utterance> {
    let mut out = Vec::new();
    for (i, (start, speaker, text)) in turns.iter().enumerate() {
        if text.is_empty() {
            continue;
        }
        let next = turns[i + 1..].iter().map(|t| t.0).find(|t| t > start).unwrap_or(start + LAST_TURN_SECONDS);
        let speaker = speaker.trim();
        let speaker = if speaker.is_empty() { "Pembicara" } else { speaker };
        out.push(Utterance::new(speaker, *start, next.max(start + 1.0), text));
    }
    out
}

/// WebVTT / SRT cues: a timing line, then the text up to the next blank line.
fn parse_cues(text: &str) -> Vec<Utterance> {
    let mut utts = Vec::new();
    let mut pos = 0;
    while let Some(m) = VTT_TIMES.captures_at(text, pos) {
        let whole = m.get(0).unwrap();
        let rest = &text[whole.end()..];
        // Python: (.+?)(?:\n\s*\n|\Z) with re.S; the body is at least one character.
        let body_end = match BLANK_LINE.find_at(rest, rest.chars().next().map_or(0, char::len_utf8)) {
            Some(b) => b.start(),
            None => rest.len(),
        };
        let body = &rest[..body_end];
        pos = whole.end() + body_end;
        if body.is_empty() {
            continue;
        }
        let (start, end) = (seconds(&m[1]), seconds(&m[2]));
        let first_line = body.split('\n').next().unwrap_or("");
        let (speaker, said) = if let Some(v) = VTT_VOICE.captures(body) {
            (v[1].to_string(), v[2].to_string())
        } else if first_line.chars().take(60).collect::<String>().contains(':') {
            let (s, t) = body.split_once(':').unwrap();
            (s.to_string(), t.to_string())
        } else {
            ("Pembicara".to_string(), body.to_string())
        };
        let said = squash(&TAG.replace_all(&said, ""));
        if !said.is_empty() {
            utts.push(Utterance::new(speaker.trim(), start, end.max(start + 0.5), &said));
        }
    }
    utts
}

pub fn parse(text: &str) -> Vec<Utterance> {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let text = text.trim_start_matches('\u{feff}');

    if text.trim_start().to_uppercase().starts_with("WEBVTT") || text.contains("-->") {
        return parse_cues(text);
    }

    let headers: Vec<_> = HEADER.captures_iter(text).collect();
    if !headers.is_empty() {
        let turns: Vec<(f64, String, String)> = headers
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let body_end = headers.get(i + 1).map_or(text.len(), |n| n.get(0).unwrap().start());
                let body = &text[m.get(0).unwrap().end()..body_end];
                (seconds(&m[1]), m[2].to_string(), squash(body))
            })
            .collect();
        return from_starts(&turns);
    }

    let inline: Vec<(f64, String, String)> =
        INLINE.captures_iter(text).map(|m| (seconds(&m[1]), m[2].to_string(), squash(&m[3]))).collect();
    if !inline.is_empty() {
        return from_starts(&inline);
    }

    let body = squash(text);
    if body.is_empty() { vec![] } else { vec![Utterance::new("Pembicara", 0.0, 60.0, &body)] }
}

pub fn load(path: &Path) -> std::io::Result<Vec<Utterance>> {
    let bytes = std::fs::read(path)?;
    Ok(parse(&String::from_utf8_lossy(&bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    const READAI: &str = "0:00 - Raka Aditya Pratama
So first, a bit of context of what is the project that I built.

 Ternyata sudah pakai kahut.

2:08 - Nadia Kusuma
But also, kalau pakai kahut kan pakai produk lain, gimana?

1:20:08 - Unidentified Speaker
Engineer atau gimana?
";

    #[test]
    fn readai_headers_with_multi_paragraph_bodies() {
        let u = parse(READAI);
        let got: Vec<_> = u.iter().map(|x| (x.speaker.as_str(), x.start, x.end)).collect();
        assert_eq!(got, [("Raka Aditya Pratama", 0.0, 128.0), ("Nadia Kusuma", 128.0, 4808.0), ("Unidentified Speaker", 4808.0, 4818.0)]);
        assert_eq!(u[0].text, "So first, a bit of context of what is the project that I built. Ternyata sudah pakai kahut.");
    }

    #[test]
    fn windows_line_endings_and_bom() {
        let u = parse(&format!("\u{feff}{}", READAI.replace('\n', "\r\n")));
        assert_eq!(u.len(), 3);
        assert_eq!(u[1].speaker, "Nadia Kusuma");
    }

    #[test]
    fn inline_turns() {
        let u = parse("[00:00:05] Budi: Halo semua\n[00:01:10] Sari: Pagi, kita mulai");
        let got: Vec<_> = u.iter().map(|x| (x.speaker.as_str(), x.start, x.text.as_str())).collect();
        assert_eq!(got, [("Budi", 5.0, "Halo semua"), ("Sari", 70.0, "Pagi, kita mulai")]);
    }

    #[test]
    fn webvtt_with_voices() {
        let vtt = "WEBVTT

00:00:01.000 --> 00:00:04.500
<v Nadia Kusuma>Oke kita mulai.</v>

00:00:05.000 --> 00:00:07.000
Raka: Siap, Bu.
";
        let u = parse(vtt);
        let got: Vec<_> = u.iter().map(|x| (x.speaker.as_str(), x.start, x.end, x.text.as_str())).collect();
        assert_eq!(got, [("Nadia Kusuma", 1.0, 4.5, "Oke kita mulai."), ("Raka", 5.0, 7.0, "Siap, Bu.")]);
    }

    #[test]
    fn srt_with_numbered_cues() {
        let srt = "1\n00:00:01,000 --> 00:00:03,000\nHalo semua\n\n2\n00:00:04,000 --> 00:00:06,000\nBudi: Siap\n";
        let u = parse(srt);
        let got: Vec<_> = u.iter().map(|x| (x.speaker.as_str(), x.start, x.text.as_str())).collect();
        assert_eq!(got, [("Pembicara", 1.0, "Halo semua"), ("Budi", 4.0, "Siap")]);
    }

    #[test]
    fn plain_text_falls_back_to_one_speaker() {
        let u = parse("Catatan rapat tanpa waktu.\nBaris kedua.");
        assert_eq!(u.len(), 1);
        assert_eq!(u[0].text, "Catatan rapat tanpa waktu. Baris kedua.");
        assert!(parse("   ").is_empty());
        assert!(parse("WEBVTT\n\n").is_empty());
    }
}
