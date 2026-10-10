//! From audio files to a finished meeting: transcript, then notulen.
//!
//! Both steps are pure orchestration over injected engines, so the flow is
//! testable without models and the app decides how engines are built.

use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::language::resolve;
use crate::minutes::lines_from;
use crate::minutes_writer::write_minutes;
use crate::prompts::Schema;
use crate::store::{Store, Update, DONE, FAILED, QUEUED, SUMMARIZING, TRANSCRIBING};
use crate::text::py_round;
use crate::transcript::{merge_tracks, Utterance, ME, OTHERS};

pub const MIC_FILE: &str = "mic.wav";
pub const SYSTEM_FILE: &str = "system.wav";
/// Speaker label for an imported single-track file.
pub const SPEAKER: &str = "Pembicara";

pub trait TranscriberLike {
    fn transcribe(&mut self, audio: &Path, speaker: &str, on_progress: &mut dyn FnMut(f64), cancelled: &dyn Fn() -> bool)
        -> Result<Vec<Utterance>>;
}

pub trait LlmLike {
    fn complete_json(&mut self, system: &str, user: &str, schema: Schema) -> Result<Value>;
}

/// How the app builds its engines; tests pass fakes.
pub trait Engines {
    fn transcriber(&self, language: &str, glossary: &[String]) -> Result<Box<dyn TranscriberLike + '_>>;
    fn llm(&self) -> Result<Box<dyn LlmLike + '_>>;
}

fn big_enough(p: &Path) -> bool {
    std::fs::metadata(p).is_ok_and(|m| m.len() > 1024)
}

/// Audio to transcribe and who is speaking on each.
pub fn tracks_for(folder: &Path) -> Vec<(PathBuf, &'static str)> {
    let (mic, system) = (folder.join(MIC_FILE), folder.join(SYSTEM_FILE));
    if mic.exists() || system.exists() {
        return [(mic, ME), (system, OTHERS)].into_iter().filter(|(p, _)| big_enough(p)).collect();
    }
    imported_audio(folder).map(|p| vec![(p, SPEAKER)]).unwrap_or_default()
}

/// The imported audio/video file of a meeting, if any.
pub fn imported_audio(folder: &Path) -> Option<PathBuf> {
    let mut imported: Vec<PathBuf> = std::fs::read_dir(folder)
        .map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    imported.retain(|p| p.is_file() && p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("import.")));
    imported.sort();
    imported.into_iter().next()
}

pub struct Job<'a> {
    pub glossary: &'a [String],
    pub skip_transcription: bool,
    /// "id", "en", or "auto" to follow what was spoken.
    pub notulen_language: &'a str,
}

pub fn process(
    meeting_id: &str,
    folder: &Path,
    store: &Store,
    engines: &dyn Engines,
    job: &Job,
    progress: &mut dyn FnMut(&str, f64),
    cancelled: &dyn Fn() -> bool,
) {
    let Some(meeting) = store.get(meeting_id) else { return };
    let mut timings = meeting.timings.clone();
    let result = (|| -> Result<()> {
        let mut utterances = meeting.transcript.clone();
        if !job.skip_transcription || utterances.is_empty() {
            store.update(meeting_id, Update { status: Some(TRANSCRIBING.into()), error: Some(String::new()), ..Default::default() })?;
            let started = Instant::now();
            utterances = transcribe(folder, &meeting.language, job.glossary, engines, progress, cancelled)?;
            timings.insert("transcribe_s".into(), json!(py_round(started.elapsed().as_secs_f64(), 1)));
            store.update(meeting_id, Update { transcript: Some(utterances.clone()), timings: Some(timings.clone()), ..Default::default() })?;
        }

        if utterances.is_empty() {
            store.update(meeting_id, Update {
                status: Some(FAILED.into()),
                error: Some("Tidak ada suara yang terdeteksi. Cek mikrofon dan audio rapat.".into()),
                ..Default::default()
            })?;
            return Ok(());
        }

        store.update(meeting_id, Update::status(SUMMARIZING))?;
        progress("summarizing", 0.0);
        let started = Instant::now();
        let spoken = utterances.iter().map(|u| u.text.as_str()).collect::<Vec<_>>().join(" ");
        let fallback = if meeting.language.is_empty() { "id" } else { &meeting.language };
        let language = resolve(job.notulen_language, &spoken, fallback);
        let notulen = {
            let mut llm = engines.llm()?;
            write_minutes(&lines_from(&utterances), &mut |s, u, k| llm.complete_json(s, u, k), &language, &mut |v| {
                progress("summarizing", v)
            })?
        };
        timings.insert("summarize_s".into(), json!(py_round(started.elapsed().as_secs_f64(), 1)));

        // Never overwrite a user's title.
        let title = (!notulen.title.is_empty() && meeting.has_default_title()).then(|| notulen.title.clone());
        store.update(meeting_id, Update {
            title,
            notulen: Some(Some(notulen)),
            status: Some(DONE.into()),
            timings: Some(timings.clone()),
            error: Some(String::new()),
            ..Default::default()
        })?;
        Ok(())
    })();

    if let Err(err) = result {
        if cancelled() {
            // App is closing: pick the meeting up again next start.
            let _ = store.update(meeting_id, Update { status: Some(QUEUED.into()), timings: Some(timings), ..Default::default() });
            return;
        }
        log::error!("Processing {meeting_id} failed: {err:#}");
        let _ = store.update(meeting_id, Update {
            status: Some(FAILED.into()),
            error: Some(err.to_string()),
            timings: Some(timings),
            ..Default::default()
        });
    }
}

fn transcribe(
    folder: &Path,
    language: &str,
    glossary: &[String],
    engines: &dyn Engines,
    progress: &mut dyn FnMut(&str, f64),
    cancelled: &dyn Fn() -> bool,
) -> Result<Vec<Utterance>> {
    let tracks = tracks_for(folder);
    if tracks.is_empty() {
        return Ok(vec![]);
    }
    let n = tracks.len() as f64;
    let mut by_speaker: Vec<(&str, Vec<Utterance>)> = Vec::new();
    {
        let mut transcriber = engines.transcriber(language, glossary)?;
        for (index, (path, speaker)) in tracks.iter().enumerate() {
            let utts = transcriber.transcribe(path, speaker, &mut |v| progress("transcribing", (index as f64 + v) / n), cancelled)?;
            by_speaker.push((speaker, utts));
        }
    }
    let take = |who: &str, list: &mut Vec<(&str, Vec<Utterance>)>| {
        list.iter().position(|(s, _)| *s == who).map(|i| list.remove(i).1).unwrap_or_default()
    };
    if by_speaker.iter().any(|(s, _)| *s == SPEAKER) {
        return Ok(take(SPEAKER, &mut by_speaker));
    }
    let mine = take(ME, &mut by_speaker);
    let theirs = take(OTHERS, &mut by_speaker);
    Ok(merge_tracks(mine, theirs))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    pub struct FakeTranscriber {
        pub script: HashMap<String, Vec<Utterance>>,
        pub calls: Rc<RefCell<Vec<(String, String)>>>,
        pub fail: Option<String>,
    }

    impl TranscriberLike for FakeTranscriber {
        fn transcribe(&mut self, audio: &Path, speaker: &str, on_progress: &mut dyn FnMut(f64), _: &dyn Fn() -> bool) -> Result<Vec<Utterance>> {
            if let Some(e) = &self.fail {
                anyhow::bail!("{e}");
            }
            self.calls.borrow_mut().push((audio.file_name().unwrap().to_string_lossy().into(), speaker.into()));
            on_progress(1.0);
            Ok(self.script.get(speaker).cloned().unwrap_or_default())
        }
    }

    /// Answers each pass of the minutes writer like a well-behaved model, citing line L1.
    pub struct FakeLlm {
        pub fail: bool,
        pub systems: Rc<RefCell<Vec<String>>>,
    }

    impl LlmLike for FakeLlm {
        fn complete_json(&mut self, system: &str, _user: &str, schema: Schema) -> Result<Value> {
            if self.fail {
                anyhow::bail!("model crashed");
            }
            self.systems.borrow_mut().push(system.into());
            Ok(match schema {
                Schema::Topics => json!({"chapters": [{"title": "Rilis", "summary": "Tim membahas rilis.", "refs": ["L1"]}],
                    "decisions": [{"text": "Rilis hari Jumat", "refs": ["L1"]}], "questions": []}),
                Schema::Actions => json!({"actions": [{"quote": "deploy", "kind": "will_do", "owner": "Budi", "task": "Deploy", "due": "", "refs": ["L1"]}]}),
                Schema::Review => json!({"items": [{"id": 1, "verdict": "follow_up", "owner": "Budi", "task": "Deploy"}]}),
                Schema::Overview => json!({"title": "Rapat rilis", "overview": "Ok", "highlights": ["Rilis Jumat"]}),
            })
        }
    }

    /// Fake engines. ``script`` is what each speaker says.
    #[derive(Default)]
    pub struct Fakes {
        pub script: HashMap<String, Vec<Utterance>>,
        pub llm_fails: bool,
        pub transcriber_fails: Option<String>,
        pub calls: Rc<RefCell<Vec<(String, String)>>>,
        pub seen: RefCell<Option<(String, Vec<String>)>>,
        pub systems: Rc<RefCell<Vec<String>>>,
    }

    impl Fakes {
        pub fn saying(pairs: &[(&str, Vec<Utterance>)]) -> Self {
            Fakes { script: pairs.iter().map(|(s, u)| (s.to_string(), u.clone())).collect(), ..Default::default() }
        }
    }

    impl Engines for Fakes {
        fn transcriber(&self, language: &str, glossary: &[String]) -> Result<Box<dyn TranscriberLike + '_>> {
            *self.seen.borrow_mut() = Some((language.into(), glossary.to_vec()));
            Ok(Box::new(FakeTranscriber { script: self.script.clone(), calls: self.calls.clone(), fail: self.transcriber_fails.clone() }))
        }
        fn llm(&self) -> Result<Box<dyn LlmLike + '_>> {
            Ok(Box::new(FakeLlm { fail: self.llm_fails, systems: self.systems.clone() }))
        }
    }

    fn setup() -> (tempfile::TempDir, Store, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("t.db")).unwrap();
        let folder = dir.path().join("m");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(MIC_FILE), vec![b'x'; 2048]).unwrap();
        std::fs::write(folder.join(SYSTEM_FILE), vec![b'x'; 2048]).unwrap();
        (dir, store, folder)
    }

    fn job(lang: &'static str) -> Job<'static> {
        Job { glossary: &[], skip_transcription: false, notulen_language: lang }
    }

    fn run(store: &Store, id: &str, folder: &Path, engines: &dyn Engines, job: &Job) -> Vec<String> {
        let mut stages = vec![];
        process(id, folder, store, engines, job, &mut |s, _| stages.push(s.to_string()), &|| false);
        stages
    }

    #[test]
    fn recorded_meeting_gets_merged_transcript_and_notulen() {
        let (_d, store, folder) = setup();
        let m = store.create("", "record", "id", "recording", None).unwrap();
        let fakes = Fakes::saying(&[
            (ME, vec![Utterance::new(ME, 5.0, 6.0, "Siap, saya deploy")]),
            (OTHERS, vec![Utterance::new(OTHERS, 0.0, 4.0, "Tolong deploy Jumat ya")]),
        ]);
        let stages = run(&store, &m.id, &folder, &fakes, &job("auto"));
        let got = store.get(&m.id).unwrap();
        assert_eq!(got.status, DONE);
        assert_eq!(got.transcript.iter().map(|u| u.speaker.as_str()).collect::<Vec<_>>(), [OTHERS, ME]);
        assert_eq!(got.title, "Rapat rilis");
        let n = got.notulen.unwrap();
        assert_eq!(n.actions[0].owner, "Budi");
        assert_eq!(n.actions[0].refs, [1]);
        assert!(got.timings.contains_key("transcribe_s") && got.timings.contains_key("summarize_s"));
        assert_eq!(stages.first().unwrap(), "transcribing");
        assert_eq!(stages.last().unwrap(), "summarizing");
    }

    #[test]
    fn user_title_is_kept() {
        let (_d, store, folder) = setup();
        let m = store.create("Rapat mingguan", "record", "id", "recording", None).unwrap();
        let fakes = Fakes::saying(&[(ME, vec![Utterance::new(ME, 0.0, 1.0, "Halo")])]);
        run(&store, &m.id, &folder, &fakes, &job("auto"));
        assert_eq!(store.get(&m.id).unwrap().title, "Rapat mingguan");
    }

    #[test]
    fn silent_meeting_fails_with_a_clear_message() {
        let (_d, store, folder) = setup();
        let m = store.create("", "record", "id", "recording", None).unwrap();
        run(&store, &m.id, &folder, &Fakes::default(), &job("auto"));
        let got = store.get(&m.id).unwrap();
        assert_eq!(got.status, FAILED);
        assert!(got.error.contains("Tidak ada suara"));
    }

    #[test]
    fn llm_failure_keeps_transcript_and_retry_skips_asr() {
        let (_d, store, folder) = setup();
        let m = store.create("", "record", "id", "recording", None).unwrap();
        let mut fakes = Fakes::saying(&[(ME, vec![Utterance::new(ME, 0.0, 1.0, "Halo")])]);
        fakes.llm_fails = true;
        run(&store, &m.id, &folder, &fakes, &job("auto"));
        let got = store.get(&m.id).unwrap();
        assert_eq!((got.status.as_str(), got.error.as_str()), (FAILED, "model crashed"));
        assert!(!got.transcript.is_empty());

        fakes.llm_fails = false;
        fakes.calls.borrow_mut().clear();
        run(&store, &m.id, &folder, &fakes, &Job { glossary: &[], skip_transcription: true, notulen_language: "auto" });
        assert!(fakes.calls.borrow().is_empty());
        assert_eq!(store.get(&m.id).unwrap().status, DONE);
    }

    #[test]
    fn glossary_and_language_reach_the_transcriber() {
        let (_d, store, folder) = setup();
        let m = store.create("", "record", "en", "recording", None).unwrap();
        let fakes = Fakes::saying(&[(ME, vec![Utterance::new(ME, 0.0, 1.0, "Hi")])]);
        let glossary = ["Lariksa".to_string()];
        run(&store, &m.id, &folder, &fakes, &Job { glossary: &glossary, skip_transcription: false, notulen_language: "auto" });
        assert_eq!(*fakes.seen.borrow(), Some(("en".to_string(), vec!["Lariksa".to_string()])));
    }

    fn language_run(text: &str, setting: &'static str) -> (String, String) {
        let (_d, store, folder) = setup();
        let m = store.create("", "record", "id", "recording", None).unwrap();
        let fakes = Fakes::saying(&[(ME, vec![Utterance::new(ME, 0.0, 5.0, text)])]);
        run(&store, &m.id, &folder, &fakes, &job(setting));
        let lang = store.get(&m.id).unwrap().notulen.unwrap().language;
        let system = fakes.systems.borrow()[0].clone();
        (lang, system)
    }

    #[test]
    fn english_speech_gives_english_notulen_even_when_audio_setting_is_indonesian() {
        let (lang, system) = language_run("Good morning everyone. We will have the meeting today about testing the application.", "auto");
        assert_eq!(lang, "en");
        assert!(system.contains("careful meeting note-taker"));
    }

    #[test]
    fn indonesian_speech_gives_indonesian_notulen() {
        let (lang, system) = language_run("Halo semuanya, kita bahas rilis minggu ini ya.", "auto");
        assert_eq!(lang, "id");
        assert!(system.contains("Bahasa Indonesia"));
    }

    #[test]
    fn fixed_notulen_language_overrides_detection() {
        let (lang, system) = language_run("Good morning, we will review the plan.", "id");
        assert_eq!(lang, "id");
        assert!(system.contains("Bahasa Indonesia"));
    }

    #[test]
    fn closing_the_app_mid_job_requeues_instead_of_failing() {
        let (_d, store, folder) = setup();
        let m = store.create("", "record", "id", "recording", None).unwrap();
        let fakes = Fakes { transcriber_fails: Some("Transkripsi dibatalkan.".into()), ..Default::default() };
        process(&m.id, &folder, &store, &fakes, &job("auto"), &mut |_, _| {}, &|| true);
        assert_eq!(store.get(&m.id).unwrap().status, QUEUED);
        assert_eq!(store.pending(), [m.id]);
    }

    #[test]
    fn tracks_for_imported_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("import.m4a"), b"x").unwrap();
        assert_eq!(tracks_for(dir.path()), [(dir.path().join("import.m4a"), SPEAKER)]);
    }

    #[test]
    fn tracks_for_skips_empty_track() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(MIC_FILE), vec![b'x'; 2048]).unwrap();
        std::fs::write(dir.path().join(SYSTEM_FILE), vec![b'x'; 44]).unwrap();
        assert_eq!(tracks_for(dir.path()), [(dir.path().join(MIC_FILE), ME)]);
    }
}
