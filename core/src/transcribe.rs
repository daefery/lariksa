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

/// Whisper's classic failure on silence: the prompt or a sign-off read back as speech.
fn is_hallucination(text: &str, prompt: &str) -> bool {
    let t = text.trim().trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase();
    t.is_empty() || (t.len() > 20 && prompt.to_lowercase().contains(&t))
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
        let args = arguments(&self.options, audio, &prefix, threads);
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
        let segments: Vec<Segment> = parse_output(&json).into_iter().filter(|s| !is_hallucination(&s.text, &prompt)).collect();
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
        assert!(s.contains("-l id") && s.contains("-dev 1") && s.contains("--vad") && s.contains("-vsd 500") && s.contains("-oj"));
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
}
