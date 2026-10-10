//! Speech to text with whisper.cpp (``whisper-cli``), tuned for Indonesian meetings.
//!
//! Three settings carry most of the accuracy:
//!
//! * the language is pinned instead of guessed from the first 30 seconds, which
//!   is how an English greeting turns forty minutes of Indonesian into nonsense;
//! * the glossary goes into the initial prompt, biasing spelling toward the
//!   team's names and jargon;
//! * VAD drops silence, which both speeds things up and stops Whisper from
//!   inventing text over quiet stretches of the system track.
//!
//! The GPU is chosen explicitly (see ``hardware``): left alone, whisper.cpp
//! picks the first GPU, which on most laptops is the slow integrated one.
//!
//! whisper.cpp cuts speech into short pieces ("Belum ada", "Ya"); those are
//! joined back into sentence-length utterances here, so echo removal, the
//! transcript view and the exports read like before.

use anyhow::{anyhow, bail, Context, Result};
use serde_json::Value;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::process;
use crate::prompts::asr_base;
use crate::transcript::Utterance;

#[derive(Debug, Clone)]
pub struct TranscribeOptions {
    pub cli: PathBuf,
    pub model: PathBuf,
    pub vad: PathBuf,
    /// whisper.cpp GPU index, or None for CPU.
    pub gpu: Option<u32>,
    /// "id", "en", or "" for auto-detect.
    pub language: String,
    pub glossary: Vec<String>,
}

pub fn build_prompt(language: &str, glossary: &[String]) -> String {
    let base = asr_base(if language.is_empty() { "id" } else { language });
    if glossary.is_empty() {
        base.to_string()
    } else {
        format!("{base} Istilah: {}.", glossary.join(", "))
    }
}

/// The command-line arguments for one track; written to a UTF-8 response file
/// so the prompt keeps non-ASCII characters (Windows would pass the ANSI code page).
pub fn arguments(o: &TranscribeOptions, audio: &Path, out_prefix: &Path, threads: usize) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-m".into(), native(&o.model),
        "-f".into(), native(audio),
        "-l".into(), if o.language.is_empty() { "auto".into() } else { o.language.clone() },
        "-t".into(), threads.to_string(),
        "--prompt".into(), build_prompt(&o.language, &o.glossary),
        "-oj".into(), "-of".into(), native(out_prefix),
        "-np".into(), "-pp".into(),
        // No text carried from one 30-second window to the next. With it, a faint
        // mic track can lock into one phrase for minutes ("Siapa yang ngajakin
        // tulip di mana" x38 on a real meeting); without it the same meeting
        // matched the earlier engine's transcript better (0.83 vs 0.64).
        "-mc".into(), "0".into(),
    ];
    if o.vad.exists() {
        // Settings that matched the previous engine's wording best on a real meeting.
        args.extend(["--vad", "-vm"].map(String::from));
        args.push(native(&o.vad));
        args.extend(["-vsd", "500", "-vp", "200"].map(String::from));
    }
    match o.gpu {
        Some(index) => args.extend(["-dev".into(), index.to_string()]),
        None => args.push("-ng".into()),
    }
    args
}

/// A path whisper-cli can open: the 8.3 short form on Windows (always ASCII),
/// because the CLI reads arguments in the ANSI code page.
fn native(path: &Path) -> String {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::{OsStrExt, OsStringExt};
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::GetShortPathNameW;
        let (dir, file) = if path.exists() { (path.to_path_buf(), None) } else { (path.parent().unwrap().to_path_buf(), path.file_name()) };
        let wide: Vec<u16> = dir.as_os_str().encode_wide().chain([0]).collect();
        let mut buf = vec![0u16; 1024];
        let n = unsafe { GetShortPathNameW(PCWSTR(wide.as_ptr()), Some(&mut buf)) } as usize;
        if n > 0 && n < buf.len() {
            let mut short = PathBuf::from(std::ffi::OsString::from_wide(&buf[..n]));
            if let Some(f) = file {
                short.push(f);
            }
            return short.to_string_lossy().into_owned();
        }
    }
    path.to_string_lossy().into_owned()
}

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// Segments from whisper-cli's ``-oj`` output.
pub fn parse_output(json: &Value) -> Vec<Segment> {
    json["transcription"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|s| {
                    let text = s["text"].as_str()?.trim().to_string();
                    let start = s["offsets"]["from"].as_f64()? / 1000.0;
                    let end = s["offsets"]["to"].as_f64()? / 1000.0;
                    (!text.is_empty()).then_some(Segment { start, end, text })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Sign-offs Whisper learned from subtitled videos and "hears" in faint or empty audio.
static FILLER: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(concat!(
        r"(?i)terima kasih (telah|sudah|udah) menonton|thanks? (you )?for watching|jangan lupa (like|subscribe)",
        r"|\bbersambung\b|sampai jumpa di (video|episode)|subtitle (oleh|by)"
    ))
    .unwrap()
});

fn words(text: &str) -> Vec<String> {
    text.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(String::from).collect()
}

/// "nge-nge-nge-nge…" or "Kita bahas. Kita bahas. Kita bahas…": one short phrase
/// repeated back to back makes up most of the segment.
fn is_loop(text: &str) -> bool {
    let w = words(text);
    if w.len() < 6 {
        return false;
    }
    (1..=3).any(|n| {
        let mut best = 1;
        for start in 0..n {
            let mut run = 1;
            let mut i = start;
            while i + 2 * n <= w.len() {
                if w[i..i + n] == w[i + n..i + 2 * n] {
                    run += 1;
                    best = best.max(run);
                } else {
                    run = 1;
                }
                i += n;
            }
        }
        best >= 4 && best * n * 10 >= w.len() * 6
    })
}

/// Whisper's failures on silence and on faint bleed from the speakers: the prompt
/// read back, a video sign-off, a phrase stuck in a loop.
fn is_hallucination(text: &str, prompt: &str) -> bool {
    let t = text.trim().trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    t.is_empty() || (t.len() > 20 && prompt.to_lowercase().contains(&t)) || FILLER.is_match(&t) || is_loop(&t)
}

/// A phrase "spoken" this slowly is not speech: people say about 2-3 words a second.
const MAX_SECONDS_PER_WORD: f64 = 5.0;
/// The same sentence of four words or more, back to back this many times, is a
/// decoding loop however fast it comes ("Aku tidak menghubungi kemurusahaan." x12).
const MAX_SENTENCE_REPEATS: usize = 6;

/// Drop hallucinated segments, including one phrase repeated three times or more
/// in a row while stretched over far more time than it takes to say ("Terima
/// kasih." x7 across ten minutes of someone else talking). People repeating
/// themselves quickly ("Belum ada, belum ada, belum ada") are kept.
pub fn drop_hallucinations(segments: Vec<Segment>, prompt: &str) -> Vec<Segment> {
    let keys: Vec<Vec<String>> = segments.iter().map(|s| words(&s.text)).collect();
    let mut repeated = vec![false; segments.len()];
    let mut i = 0;
    while i < keys.len() {
        let mut j = i + 1;
        while j < keys.len() && keys[j] == keys[i] {
            j += 1;
        }
        let spoken_words = (keys[i].len() * (j - i)) as f64;
        let seconds = segments[j - 1].end - segments[i].start;
        let stretched = j - i >= 3 && seconds > spoken_words * MAX_SECONDS_PER_WORD;
        let looping = j - i >= MAX_SENTENCE_REPEATS && keys[i].len() >= 4;
        if stretched || looping {
            repeated[i..j].iter_mut().for_each(|r| *r = true);
        }
        i = j;
    }
    segments
        .into_iter()
        .zip(repeated)
        .filter(|(s, rep)| !rep && !is_hallucination(&s.text, prompt))
        .map(|(s, _)| s)
        .collect()
}

/// Join short pieces into utterances: same speaker, under a second apart, up to
/// a sentence end once there is enough text, never past ``max_chars``.
pub fn merge_segments(segments: &[Segment], max_gap: f64, max_chars: usize) -> Vec<Segment> {
    let mut out: Vec<Segment> = Vec::new();
    for s in segments {
        if let Some(last) = out.last_mut() {
            let ends_sentence = last.text.ends_with(['.', '?', '!', '…']);
            let long_enough = last.text.chars().count() >= max_chars / 3;
            let fits = last.text.chars().count() + 1 + s.text.chars().count() <= max_chars;
            if s.start - last.end <= max_gap && fits && !(ends_sentence && long_enough) {
                last.end = s.end;
                last.text = format!("{} {}", last.text, s.text);
                continue;
            }
        }
        out.push(s.clone());
    }
    out
}

pub struct Transcriber {
    pub options: TranscribeOptions,
}

impl Transcriber {
    pub fn new(options: TranscribeOptions) -> Result<Self> {
        if !options.cli.exists() {
            bail!("Mesin transkripsi belum terpasang. Buka Pengaturan.");
        }
        if !options.model.exists() {
            bail!("Model transkripsi belum diunduh. Buka Pengaturan.");
        }
        Ok(Self { options })
    }

    /// "Deteksi otomatis": find the language from speech only. whisper-cli guesses
    /// from the first 30 seconds, which on a mic track are often near-silent; on a
    /// real Indonesian meeting that guess was Spanish (p=0.87) without speech
    /// detection and Indonesian (p=0.998) with it. Falls back to Indonesian.
    fn detect_language(&self, audio: &Path, work: &Path, threads: usize) -> String {
        let mut args: Vec<String> = vec!["-m".into(), native(&self.options.model), "-f".into(), native(audio), "-l".into(), "auto".into(),
                                         "-dl".into(), "-oj".into(), "-np".into(), "-t".into(), threads.to_string(),
                                         "-of".into(), native(&work.join("lang"))];
        if self.options.vad.exists() {
            args.extend(["--vad".into(), "-vm".into(), native(&self.options.vad)]);
        }
        match self.options.gpu {
            Some(i) => args.extend(["-dev".into(), i.to_string()]),
            None => args.push("-ng".into()),
        }
        let file = work.join("lang-args.txt");
        let detected = std::fs::write(&file, args.join("\n"))
            .ok()
            .and_then(|_| process::output_within(&self.options.cli, &[&format!("@{}", native(&file))], Duration::from_secs(120)).ok())
            .and_then(|_| std::fs::read(work.join("lang.json")).ok())
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .and_then(|j| j["result"]["language"].as_str().map(String::from))
            .filter(|l| !l.is_empty());
        log::info!("Detected language of {}: {}", audio.display(), detected.as_deref().unwrap_or("none, using id"));
        detected.unwrap_or_else(|| "id".into())
    }

    pub fn transcribe(
        &self,
        audio: &Path,
        speaker: &str,
        on_progress: &mut dyn FnMut(f64),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Vec<Utterance>> {
        let work = audio.parent().unwrap().join(format!(".asr-{speaker}"));
        std::fs::create_dir_all(&work)?;
        let prefix = work.join("out");
        // Imported audio/video (mp3, m4a, mp4, webm...) is decoded to 16 kHz WAV first;
        // whisper-cli itself reads only WAV, MP3, FLAC and OGG reliably.
        let decoded;
        let audio = if audio.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav")) {
            audio
        } else {
            decoded = work.join("input.wav");
            crate::audio::decode_to_wav(audio, &decoded)?;
            decoded.as_path()
        };
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 8);
        let mut options = self.options.clone();
        if options.language.is_empty() {
            options.language = self.detect_language(audio, &work, threads);
        }
        let args = arguments(&options, audio, &prefix, threads);
        std::fs::write(work.join("args.txt"), args.join("\n"))?;

        let mut cmd = process::command(&self.options.cli);
        cmd.arg(format!("@{}", native(&work.join("args.txt")))).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::piped());
        let mut child = process::spawn(cmd).context("whisper-cli tidak bisa dijalankan")?;

        // whisper-cli prints "progress =  45%" to stderr; keep the last lines for errors.
        let progress = Arc::new(Mutex::new((0.0f64, Vec::<String>::new())));
        let stderr = child.stderr.take().unwrap();
        let reader = {
            let progress = progress.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                    let mut p = progress.lock().unwrap();
                    if let Some(pct) = line.split("progress =").nth(1).and_then(|s| s.trim().trim_end_matches('%').trim().parse::<f64>().ok()) {
                        p.0 = pct / 100.0;
                    } else {
                        p.1.push(line);
                        if p.1.len() > 20 {
                            p.1.remove(0);
                        }
                    }
                }
            })
        };
        let status = loop {
            if cancelled() {
                process::stop(&mut child);
                let _ = std::fs::remove_dir_all(&work);
                bail!("Transkripsi dibatalkan.");
            }
            if let Some(status) = child.try_wait()? {
                break status;
            }
            on_progress(progress.lock().unwrap().0.min(0.99));
            std::thread::sleep(Duration::from_millis(250));
        };
        let _ = reader.join();
        if !status.success() {
            let tail = progress.lock().unwrap().1.join("\n");
            let _ = std::fs::remove_dir_all(&work);
            bail!("Transkripsi gagal ({status}).\n{tail}");
        }
        let json: Value = serde_json::from_slice(&std::fs::read(prefix.with_extension("json")).map_err(|e| anyhow!("Hasil transkripsi tidak ada: {e}"))?)?;
        let _ = std::fs::remove_dir_all(&work);
        on_progress(1.0);

        let prompt = build_prompt(&self.options.language, &self.options.glossary);
        let segments = drop_hallucinations(parse_output(&json), &prompt);
        let utterances: Vec<Utterance> =
            merge_segments(&segments, 1.0, 300).into_iter().map(|s| Utterance::new(speaker, s.start, s.end, &s.text)).collect();
        log::info!("{speaker}: {} utterances from {}", utterances.len(), audio.display());
        Ok(utterances)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn seg(a: f64, b: f64, t: &str) -> Segment {
        Segment { start: a, end: b, text: t.into() }
    }

    #[test]
    fn prompt_carries_glossary() {
        assert_eq!(build_prompt("id", &[]), "Rapat kerja tim. Kita bahas progress, deadline, budget, dan follow up minggu ini.");
        assert!(build_prompt("en", &["Lariksa".into(), "OKR".into()]).ends_with("Istilah: Lariksa, OKR."));
        assert!(build_prompt("", &[]).starts_with("Rapat kerja tim"));
    }

    #[test]
    fn arguments_pin_language_gpu_and_vad() {
        let dir = tempfile::tempdir().unwrap();
        let vad = dir.path().join("vad.bin");
        std::fs::write(&vad, b"x").unwrap();
        let o = TranscribeOptions { cli: "w".into(), model: "m.bin".into(), vad, gpu: Some(1), language: "id".into(), glossary: vec![] };
        let a = arguments(&o, Path::new("mic.wav"), Path::new("out"), 8);
        let s = a.join(" ");
        assert!(s.contains("-l id") && s.contains("-dev 1") && s.contains("--vad") && s.contains("-vsd 500") && s.contains("-oj") && s.contains("-mc 0"));
        let cpu = TranscribeOptions { gpu: None, language: "".into(), vad: "missing".into(), ..o };
        let s = arguments(&cpu, Path::new("mic.wav"), Path::new("out"), 8).join(" ");
        assert!(s.contains("-ng") && s.contains("-l auto") && !s.contains("--vad"));
    }

    #[test]
    fn parses_whisper_cli_json() {
        let out = json!({"transcription": [
            {"timestamps": {"from": "00:00:00,000", "to": "00:00:03,000"}, "offsets": {"from": 0, "to": 3000}, "text": " berarti kan"},
            {"offsets": {"from": 3000, "to": 6500}, "text": "  "},
            {"offsets": {"from": 7000, "to": 9000}, "text": " Ya."}]});
        assert_eq!(parse_output(&out), [seg(0.0, 3.0, "berarti kan"), seg(7.0, 9.0, "Ya.")]);
    }

    #[test]
    fn short_pieces_join_into_sentences() {
        let pieces = [seg(0.0, 1.0, "Belum ada"), seg(1.2, 2.0, "yang"), seg(2.1, 3.0, "masuk."), seg(6.0, 7.0, "Ya")];
        let out = merge_segments(&pieces, 1.0, 300);
        assert_eq!(out, [seg(0.0, 3.0, "Belum ada yang masuk."), seg(6.0, 7.0, "Ya")]);
    }

    #[test]
    fn long_runs_break_at_sentence_ends_and_size() {
        let s = "Kalimat ini cukup panjang untuk mengisi satu utterance penuh dengan kata.";
        let pieces: Vec<Segment> = (0..10).map(|i| seg(i as f64, i as f64 + 0.9, s)).collect();
        let out = merge_segments(&pieces, 1.0, 300);
        assert!(out.len() > 1);
        assert!(out.iter().all(|u| u.text.chars().count() <= 300));
        assert_eq!(out.iter().map(|u| u.text.clone()).collect::<Vec<_>>().join(" "), [s; 10].join(" "));
    }

    #[test]
    fn prompt_echo_is_dropped() {
        let p = build_prompt("id", &[]);
        assert!(is_hallucination("Rapat kerja tim. Kita bahas progress, deadline, budget", &p));
        assert!(is_hallucination(" ... ", &p));
        assert!(!is_hallucination("Kita bahas progress", &p)); // short real speech that overlaps the prompt stays
    }

    // Seen on a laptop mic picking up the meeting from the speakers (10 Oct 2026).
    #[test]
    fn bleed_hallucinations_are_dropped() {
        let p = build_prompt("id", &[]);
        for junk in [
            "Jadi, tadi saya mau nge-nge-nge-nge-nge-nge-nge-nge-nge-nge-nge-nge-nge-nge.",
            "Terima kasih. Terima kasih. Terima kasih. Terima kasih. Terima kasih. Terima kasih.",
            "Kita bahas. Kita bahas. Kita bahas. Kita bahas. Kita bahas.",
            "Terima kasih telah menonton!",
            "Bersambung, ya. Bersambung.",
        ] {
            assert!(is_hallucination(junk, &p), "{junk}");
        }
        for real in [
            "Ya. Ya. Mas Rijal. Ya. Oke. Oke. Oke. Oke. Itu aja dari saya. Oke. Itu aja kali dari saya, next.",
            "Kenapa? Banyak banget. Kenapa tuh. Jadi. Banyak banget itu. Banyak banget. Iya.",
            "Dari saya sih clear sih Bu. Terima kasih banyak Bu.",
            "unsubscribe nya aku udah tes di staging juga",
            "Oke, oke. Bisa, bisa, bisa. Oke, itu aja.",
        ] {
            assert!(!is_hallucination(real, &p), "{real}");
        }
    }

    #[test]
    fn repeated_segments_are_dropped_but_short_acknowledgements_stay() {
        let p = build_prompt("id", &[]);
        let mut segs = vec![seg(0.0, 3.0, "Ya, cipta-cipta")];
        segs.extend((0..7).map(|i| seg(50.0 + i as f64 * 80.0, 100.0 + i as f64 * 80.0, "Terima kasih.")));
        segs.extend([seg(700.0, 701.0, "Ya"), seg(701.0, 702.0, "Ya"), seg(702.0, 703.0, "Ya"), seg(704.0, 706.0, "Terima kasih.")]);
        // A sentence looping quickly is dropped too.
        segs.extend((0..12).map(|i| seg(65.0 + i as f64 * 3.0, 67.0 + i as f64 * 3.0, "Aku tidak menghubungi kemurusahaan.")));
        // Real, quick repetition (seen in the Daily Standup): kept.
        segs.extend([seg(374.0, 376.0, "jawab aja dong Ferry"), seg(376.0, 377.0, "jawab aja dong Ferry"), seg(377.0, 378.0, "jawab aja dong Ferry")]);
        let kept: Vec<String> = drop_hallucinations(segs, &p).into_iter().map(|s| s.text).collect();
        assert_eq!(kept, ["Ya, cipta-cipta", "Ya", "Ya", "Ya", "Terima kasih.", "jawab aja dong Ferry", "jawab aja dong Ferry", "jawab aja dong Ferry"]);
    }
}
