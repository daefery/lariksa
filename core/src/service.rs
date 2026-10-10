//! The application: one object the UI talks to.
//!
//! Owns the store, the recorder, a single background worker (one heavy job at a
//! time, so a laptop never runs two models at once) and asset downloads. Every
//! public method returns plain JSON, so the UI bridge stays thin. The JSON shapes
//! are exactly those the UI used with the Python version.

use serde_json::{json, Map, Value};
use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use crate::audio::repair_wav;
use crate::catalog::{self, Asset};
use crate::hardware::{self, Device};
use crate::llm::LocalLlm;
use crate::media::MediaServer;
use crate::minutes::{lines_from, Action, Chapter, Minutes, Point};
use crate::pipeline::{self, Engines, Job as PipelineJob, LlmLike, TranscriberLike, MIC_FILE, SYSTEM_FILE};
use crate::paths::Paths;
use crate::prompts::{labels, Schema};
use crate::recorder::{AudioRecorder, Recorder};
use crate::render::{to_markdown, to_whatsapp};
use crate::store::{glossary_of, human_date_of, setting_str, Store, Update, DONE, FAILED, QUEUED, RECORDING};
use crate::text::{py_round, squash, truthy};
use crate::transcribe::{TranscribeOptions, Transcriber};
use crate::transcript::Utterance;
use crate::transcript_import::{self, TEXT_TYPES};

pub const AUDIO_TYPES: [&str; 9] = [".wav", ".mp3", ".m4a", ".mp4", ".webm", ".ogg", ".flac", ".aac", ".mkv"];

/// Audio/video is transcribed; a transcript file (Read.ai, Meet, Zoom, VTT) skips straight to the notulen.
pub fn import_types() -> Vec<&'static str> {
    AUDIO_TYPES.iter().chain(TEXT_TYPES.iter()).copied().collect()
}

fn ext_of(path: &Path) -> String {
    path.extension().map(|e| format!(".{}", e.to_string_lossy().to_lowercase())).unwrap_or_default()
}

fn ok() -> Value {
    json!({"ok": true})
}

fn fail(msg: impl std::fmt::Display) -> Value {
    json!({"ok": false, "error": msg.to_string()})
}

#[derive(Clone)]
struct Job {
    meeting_id: String,
    retry_summary: bool,
}

/// What the worker and downloader are doing right now, for the UI.
#[derive(Default, Clone)]
pub struct Activity {
    pub meeting_id: String,
    pub stage: String,
    pub progress: f64,
    pub downloads: Map<String, Value>,
}

#[derive(Default)]
struct Queue {
    jobs: VecDeque<Job>,
}

/// Builds the real engines: whisper.cpp and llama.cpp on the chosen GPU.
pub struct RealEngines {
    paths: Paths,
    whisper: Asset,
    gpu: Option<Device>,
}

struct WhisperEngine(Transcriber);

impl TranscriberLike for WhisperEngine {
    fn transcribe(&mut self, audio: &Path, speaker: &str, p: &mut dyn FnMut(f64), c: &dyn Fn() -> bool) -> anyhow::Result<Vec<Utterance>> {
        self.0.transcribe(audio, speaker, p, c)
    }
}

impl LlmLike for LocalLlm {
    fn complete_json(&mut self, system: &str, user: &str, schema: Schema) -> anyhow::Result<Value> {
        LocalLlm::complete_json(self, system, user, schema)
    }
}

impl Engines for RealEngines {
    fn transcriber(&self, language: &str, glossary: &[String]) -> anyhow::Result<Box<dyn TranscriberLike + '_>> {
        let runtime = catalog::asset(catalog::WHISPER_RUNTIME).unwrap();
        let vad = catalog::asset(catalog::VAD_MODEL).unwrap();
        // whisper.cpp numbers GPUs like llama.cpp lists them: Vulkan1 -> 1.
        let gpu = self.gpu.as_ref().and_then(Device::index);
        let options = TranscribeOptions {
            cli: runtime.path(&self.paths).join(catalog::exe("whisper-cli")),
            model: self.whisper.path(&self.paths).join(&self.whisper.marker),
            vad: vad.path(&self.paths).join(&vad.marker),
            gpu,
            language: language.to_string(),
            glossary: glossary.to_vec(),
        };
        Ok(Box::new(WhisperEngine(Transcriber::new(options)?)))
    }

    fn llm(&self) -> anyhow::Result<Box<dyn LlmLike + '_>> {
        let llama = catalog::asset(catalog::LLAMA_RUNTIME).unwrap();
        let model = catalog::asset(catalog::SUMMARY_MODEL).unwrap();
        let server = llama.path(&self.paths).join(catalog::exe("llama-server"));
        // Device memory changes as other apps run; ask again for each notulen.
        let devices = hardware::list_devices(&server);
        let device = hardware::choose_device(&devices, hardware::LLM_NEED_MIB).cloned();
        log::info!("llama.cpp devices: {devices:?} -> {:?}", device.as_ref().map(|d| &d.name));
        Ok(Box::new(LocalLlm::start(&server, &model.path(&self.paths).join(&model.marker), device.as_ref())?))
    }
}

/// How the service gets its engines; tests replace it.
pub type EngineFactory = Box<dyn Fn(&LariksaService, &Map<String, Value>) -> anyhow::Result<Box<dyn Engines>> + Send + Sync>;

pub struct LariksaService {
    pub paths: Paths,
    pub store: Store,
    pub recorder: Box<dyn AudioRecorder>,
    activity: Mutex<Activity>,
    recording_id: Mutex<String>,
    queue: Mutex<Queue>,
    wake: Condvar,
    cancel: AtomicBool,
    downloading: AtomicBool,
    record_lock: Mutex<()>,
    media: Option<MediaServer>,
    gpu: OnceLock<Option<Device>>,
    engines: EngineFactory,
}

impl LariksaService {
    pub fn new(paths: Paths) -> anyhow::Result<Arc<Self>> {
        Self::with(paths, Box::new(Recorder::default()), Box::new(|svc, settings| svc.real_engines(settings)))
    }

    pub fn with(paths: Paths, recorder: Box<dyn AudioRecorder>, engines: EngineFactory) -> anyhow::Result<Arc<Self>> {
        let store = Store::open(&paths.database())?;
        let recordings = paths.recordings();
        let media = MediaServer::start(Arc::new(move |id: &str| recordings.join(id)))
            .map_err(|e| log::error!("Audio player server unavailable: {e}"))
            .ok();
        let _ = catalog::link_vulkan(&paths);
        let svc = Arc::new(LariksaService {
            paths,
            store,
            recorder,
            activity: Default::default(),
            recording_id: Default::default(),
            queue: Default::default(),
            wake: Condvar::new(),
            cancel: AtomicBool::new(false),
            downloading: AtomicBool::new(false),
            record_lock: Mutex::new(()),
            media,
            gpu: OnceLock::new(),
            engines,
        });
        svc.recover();
        svc.detect_gpu_in_background();
        let worker = svc.clone();
        std::thread::Builder::new().name("lariksa-worker".into()).spawn(move || worker.worker())?;
        Ok(svc)
    }

    // -- startup ---------------------------------------------------------

    /// Rescue meetings cut off by a crash or a closed laptop lid.
    fn recover(&self) {
        for id in self.store.interrupted_recordings() {
            let folder = self.paths.meeting_dir(&id);
            let mut duration: f64 = 0.0;
            for name in [MIC_FILE, SYSTEM_FILE] {
                if folder.join(name).exists() {
                    let _ = repair_wav(&folder.join(name));
                    let bytes = std::fs::metadata(folder.join(name)).map_or(0, |m| m.len().saturating_sub(44));
                    duration = duration.max(bytes as f64 / 2.0 / crate::audio::TARGET_RATE as f64);
                }
            }
            let _ = self.store.update(&id, Update { status: Some(QUEUED.into()), duration: Some(py_round(duration, 1)), ..Default::default() });
        }
        for id in self.store.pending() {
            self.enqueue(Job { meeting_id: id, retry_summary: false });
        }
    }

    fn enqueue(&self, job: Job) {
        self.queue.lock().unwrap().jobs.push_back(job);
        self.wake.notify_one();
    }

    pub fn queue_len(&self) -> usize {
        self.queue.lock().unwrap().jobs.len()
    }

    pub fn activity(&self) -> Activity {
        self.activity.lock().unwrap().clone()
    }

    /// The GPU Whisper and the notulen run on, detected once per session.
    ///
    /// The device list comes from llama.cpp, so nothing is cached until its
    /// runtime is installed: a first run detects the GPU right after the download.
    fn gpu(&self) -> Option<&Device> {
        if let Some(cached) = self.gpu.get() {
            return cached.as_ref();
        }
        let llama = catalog::asset(catalog::LLAMA_RUNTIME).unwrap();
        if !llama.installed(&self.paths) {
            return None;
        }
        let server = llama.path(&self.paths).join(catalog::exe("llama-server"));
        let device = hardware::choose_device(&hardware::list_devices(&server), hardware::WHISPER_NEED_MIB).cloned();
        log::info!("GPU for transcription and notulen: {}", hardware::label(device.as_ref()));
        self.gpu.get_or_init(|| device).as_ref()
    }

    /// Start GPU detection off the UI thread; listing devices takes a few seconds.
    fn detect_gpu_in_background(self: &Arc<Self>) {
        if self.gpu.get().is_none() && catalog::asset(catalog::LLAMA_RUNTIME).unwrap().installed(&self.paths) {
            let svc = self.clone();
            let _ = std::thread::Builder::new().name("lariksa-gpu".into()).spawn(move || {
                svc.gpu();
            });
        }
    }

    /// "GPU (...)" once detected; "CPU" until then (status never waits for detection).
    fn compute(&self) -> (String, String) {
        match self.gpu.get() {
            Some(Some(d)) => (hardware::label(Some(d)), d.description.clone()),
            _ => ("CPU".into(), String::new()),
        }
    }

    // -- status ----------------------------------------------------------

    pub fn whisper_asset(settings: &Map<String, Value>) -> Asset {
        let choice = setting_str(settings, "whisper_model");
        // Turbo everywhere by default: fast on a laptop GPU, and faster than real time on a modern CPU.
        if choice == catalog::WHISPER_SMALL {
            catalog::asset(catalog::WHISPER_SMALL).unwrap()
        } else {
            catalog::asset(catalog::WHISPER_TURBO).unwrap()
        }
    }

    pub fn status(&self) -> Value {
        let settings = self.store.settings();
        let assets: Map<String, Value> = catalog::assets()
            .into_iter()
            .map(|a| (a.key.to_string(), json!({"label": a.label, "size": a.size(), "installed": a.installed(&self.paths)})))
            .collect();
        let whisper = Self::whisper_asset(&settings);
        let needed = [whisper.key, catalog::VAD_MODEL, catalog::SUMMARY_MODEL, catalog::LLAMA_RUNTIME, catalog::WHISPER_RUNTIME];
        let ready = needed.iter().all(|k| assets[*k]["installed"] == true);
        let recording = self.recorder.recording();
        let activity = self.activity();
        let (label, gpu) = self.compute();
        json!({
            "recording": recording,
            "recording_id": *self.recording_id.lock().unwrap(),
            "elapsed": py_round(self.recorder.elapsed(), 1),
            "levels": self.recorder.levels(),
            "silent": if recording { self.recorder.silent_tracks() } else { vec![] },
            "devices": self.recorder.devices(),
            "activity": {
                "meeting_id": activity.meeting_id,
                "stage": activity.stage,
                "progress": py_round(activity.progress, 3),
                "downloads": activity.downloads,
            },
            "queue": self.queue_len(),
            "compute": {"label": label, "device": if gpu.is_empty() { "cpu" } else { "gpu" }, "gpu": gpu},
            "assets": assets,
            "needed": needed,
            "ready": ready,
            "settings": settings,
        })
    }

    // -- downloads -------------------------------------------------------

    pub fn download(self: &Arc<Self>, keys: Vec<String>) -> Value {
        let unknown: Vec<&String> = keys.iter().filter(|k| catalog::asset(k).is_none()).collect();
        if !unknown.is_empty() {
            return fail(format!("Aset tidak dikenal: {unknown:?}"));
        }
        if self.downloading.swap(true, Ordering::SeqCst) {
            return fail("Unduhan lain sedang berjalan.");
        }
        let svc = self.clone();
        std::thread::spawn(move || {
            for key in keys {
                let asset = catalog::asset(&key).unwrap();
                let set = |done: u64, total: u64, error: &str| {
                    svc.activity.lock().unwrap().downloads.insert(
                        key.clone(),
                        json!({"label": asset.label, "done": done, "total": total, "error": error}),
                    );
                };
                set(0, asset.size(), "");
                let result = catalog::install(&asset, &svc.paths, &mut |d, t| set(d, t, ""), &|| svc.cancel.load(Ordering::SeqCst));
                match result {
                    Ok(_) => {
                        svc.activity.lock().unwrap().downloads.remove(&key);
                        svc.detect_gpu_in_background(); // the llama.cpp runtime brings the device list
                    }
                    Err(e) => {
                        log::error!("Download {key} failed: {e:#}");
                        set(0, asset.size(), &e.to_string());
                        break;
                    }
                }
            }
            svc.downloading.store(false, Ordering::SeqCst);
        });
        ok()
    }

    // -- recording -------------------------------------------------------

    pub fn start_recording(&self, title: &str) -> Value {
        // The main window button, the pill and the global shortcut can all race here.
        let _guard = self.record_lock.lock().unwrap();
        if self.recorder.recording() {
            return fail("Rekaman sudah berjalan.");
        }
        let settings = self.store.settings();
        let meeting = match self.store.create(title.trim(), "record", &setting_str(&settings, "language"), RECORDING, None) {
            Ok(m) => m,
            Err(e) => return fail(e),
        };
        match self.recorder.start(&self.paths.meeting_dir(&meeting.id)) {
            Ok(devices) => {
                *self.recording_id.lock().unwrap() = meeting.id.clone();
                json!({"ok": true, "meeting_id": meeting.id, "devices": devices})
            }
            Err(e) => {
                let _ = self.store.delete(&meeting.id);
                let _ = std::fs::remove_dir_all(self.paths.meeting_dir(&meeting.id));
                fail(e)
            }
        }
    }

    pub fn stop_recording(&self) -> Value {
        let _guard = self.record_lock.lock().unwrap();
        if !self.recorder.recording() {
            return fail("Tidak ada rekaman yang berjalan.");
        }
        let duration = self.recorder.stop();
        let id = std::mem::take(&mut *self.recording_id.lock().unwrap());
        let _ = self.store.update(&id, Update { duration: Some(py_round(duration, 1)), status: Some(QUEUED.into()), ..Default::default() });
        self.enqueue(Job { meeting_id: id.clone(), retry_summary: false });
        json!({"ok": true, "meeting_id": id})
    }

    pub fn import_file(&self, source: &Path, title: &str) -> Value {
        let ext = ext_of(source);
        if !source.is_file() || !import_types().contains(&ext.as_str()) {
            return fail("Format file tidak didukung.");
        }
        let settings = self.store.settings();
        let mut transcript = vec![];
        if TEXT_TYPES.contains(&ext.as_str()) {
            transcript = transcript_import::load(source).unwrap_or_default();
            if transcript.is_empty() {
                return fail("Transkrip kosong atau formatnya tidak dikenali.");
            }
        }
        let stem = source.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let title = if title.trim().is_empty() { stem } else { title.trim().to_string() };
        let meeting = match self.store.create(&title, "import", &setting_str(&settings, "language"), QUEUED, None) {
            Ok(m) => m,
            Err(e) => return fail(e),
        };
        if !transcript.is_empty() {
            let duration = transcript.iter().map(|u| u.end).fold(0.0, f64::max);
            let _ = self.store.update(&meeting.id, Update { transcript: Some(transcript), duration: Some(py_round(duration, 1)), ..Default::default() });
        } else {
            let folder = self.paths.meeting_dir(&meeting.id);
            if let Err(e) = std::fs::create_dir_all(&folder).and_then(|_| std::fs::copy(source, folder.join(format!("import{ext}")))) {
                let _ = self.store.delete(&meeting.id);
                return fail(e);
            }
        }
        self.enqueue(Job { meeting_id: meeting.id.clone(), retry_summary: TEXT_TYPES.contains(&ext.as_str()) });
        json!({"ok": true, "meeting_id": meeting.id})
    }

    // -- meetings --------------------------------------------------------

    pub fn meetings(&self, query: &str) -> Value {
        Value::Array(self.store.list(query).iter().map(|m| m.summary_value()).collect())
    }

    pub fn meeting(&self, id: &str) -> Value {
        let Some(m) = self.store.get(id) else { return Value::Null };
        let mut data = m.to_value();
        let o = data.as_object_mut().unwrap();
        // The numbered line view the notulen references point into.
        let lines: Vec<Value> = lines_from(&m.transcript)
            .iter()
            .map(|l| json!({"n": l.n, "start": l.start, "speaker": l.speaker, "text": l.text}))
            .collect();
        o.insert("lines".into(), Value::Array(lines));
        let has_audio = self.has_audio(id);
        o.insert("has_audio".into(), json!(has_audio));
        let url = match (&self.media, has_audio) {
            (Some(media), true) => media.url(id),
            _ => String::new(),
        };
        o.insert("audio_url".into(), json!(url));
        if let Some(n) = &m.notulen {
            let date = human_date_of(&m.created_at);
            o.insert("labels".into(), json!(labels(&n.language)));
            o.insert("markdown".into(), json!(to_markdown(n, &date, &m.title)));
            o.insert("whatsapp".into(), json!(to_whatsapp(n, &date, &m.title)));
        }
        data
    }

    fn has_audio(&self, id: &str) -> bool {
        let folder = self.paths.meeting_dir(id);
        std::fs::read_dir(folder).is_ok_and(|rd| {
            rd.filter_map(Result::ok).any(|e| {
                let p = e.path();
                AUDIO_TYPES.contains(&ext_of(&p).as_str()) && e.metadata().is_ok_and(|m| m.len() > 1024)
            })
        })
    }

    pub fn rename(&self, id: &str, title: &str) -> Value {
        let title = title.trim();
        if title.is_empty() {
            return fail("Judul tidak boleh kosong.");
        }
        match self.store.update(id, Update { title: Some(title.into()), ..Default::default() }) {
            Ok(_) => ok(),
            Err(e) => fail(e),
        }
    }

    /// Apply UI edits. Only text, owners, due dates and done flags change;
    /// anchors (refs, start times), speakers and the language always come from storage.
    pub fn save_notulen(&self, id: &str, data: &Value) -> Value {
        let Some(stored) = self.store.get(id).and_then(|m| m.notulen) else {
            return fail("Notulen belum tersedia.");
        };
        match self.store.update(id, Update { notulen: Some(Some(merge_edits(&stored, data))), ..Default::default() }) {
            Ok(_) => ok(),
            Err(e) => fail(e),
        }
    }

    pub fn retry(&self, id: &str, summary_only: bool) -> Value {
        let Some(m) = self.store.get(id).filter(|m| m.status == DONE || m.status == FAILED) else {
            return fail("Rapat ini sedang diproses.");
        };
        let _ = self.store.update(id, Update { status: Some(QUEUED.into()), error: Some(String::new()), ..Default::default() });
        self.enqueue(Job { meeting_id: id.into(), retry_summary: summary_only && !m.transcript.is_empty() });
        ok()
    }

    pub fn delete(&self, id: &str) -> Value {
        if id == *self.recording_id.lock().unwrap() || id == self.activity().meeting_id {
            return fail("Rapat ini sedang direkam atau diproses.");
        }
        let _ = self.store.delete(id);
        let _ = std::fs::remove_dir_all(self.paths.meeting_dir(id));
        ok()
    }

    pub fn save_settings(&self, values: &Map<String, Value>) -> anyhow::Result<Map<String, Value>> {
        self.store.save_settings(values)
    }

    pub fn export_docx(&self, id: &str, target: &Path) -> Value {
        let Some(m) = self.store.get(id).filter(|m| m.notulen.is_some()) else {
            return fail("Notulen belum tersedia.");
        };
        match crate::export::write_docx(&m, target, &human_date_of(&m.created_at)) {
            Ok(_) => json!({"ok": true, "path": target}),
            Err(e) => fail(e),
        }
    }

    pub fn export_markdown(&self, id: &str, target: &Path) -> Value {
        let Some(m) = self.store.get(id) else { return fail("Rapat tidak ditemukan.") };
        match std::fs::write(target, crate::export::full_markdown(&m, &human_date_of(&m.created_at))) {
            Ok(_) => json!({"ok": true, "path": target}),
            Err(e) => fail(e),
        }
    }

    // -- worker ----------------------------------------------------------

    fn real_engines(&self, settings: &Map<String, Value>) -> anyhow::Result<Box<dyn Engines>> {
        Ok(Box::new(RealEngines { paths: self.paths.clone(), whisper: Self::whisper_asset(settings), gpu: self.gpu().cloned() }))
    }

    fn worker(self: Arc<Self>) {
        loop {
            let job = {
                let mut q = self.queue.lock().unwrap();
                loop {
                    if self.cancel.load(Ordering::SeqCst) {
                        return;
                    }
                    if let Some(job) = q.jobs.pop_front() {
                        break job;
                    }
                    q = self.wake.wait(q).unwrap();
                }
            };
            let Some(m) = self.store.get(&job.meeting_id).filter(|m| m.status != RECORDING) else { continue };
            let settings = self.store.settings();
            *self.activity.lock().unwrap() = Activity { meeting_id: m.id.clone(), stage: "queued".into(), downloads: self.activity().downloads, ..Default::default() };

            let run = || -> anyhow::Result<()> {
                let engines = (self.engines)(&self, &settings)?;
                let glossary = glossary_of(&settings);
                let job = PipelineJob {
                    glossary: &glossary,
                    skip_transcription: job.retry_summary,
                    notulen_language: &setting_str(&settings, "notulen_language"),
                };
                pipeline::process(
                    &m.id,
                    &self.paths.meeting_dir(&m.id),
                    &self.store,
                    engines.as_ref(),
                    &job,
                    &mut |stage, value| {
                        let mut a = self.activity.lock().unwrap();
                        a.stage = stage.into();
                        a.progress = value;
                    },
                    &|| self.cancel.load(Ordering::SeqCst),
                );
                Ok(())
            };
            // Never let one meeting kill the worker for the session.
            if let Err(e) = run() {
                log::error!("Worker failed on {}: {e:#}", m.id);
                let _ = self.store.update(&m.id, Update { status: Some(FAILED.into()), error: Some(e.to_string()), ..Default::default() });
            }
            let mut a = self.activity.lock().unwrap();
            a.meeting_id.clear();
            a.stage.clear();
            a.progress = 0.0;
        }
    }

    /// Save any recording; interrupted processing resumes on next start.
    pub fn shutdown(&self) {
        if self.recorder.recording() {
            self.stop_recording();
        }
        self.cancel.store(true, Ordering::SeqCst);
        self.wake.notify_all();
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// Overlay editable fields from the UI onto the stored minutes.
///
/// Lists are matched by position; the UI may delete items (shorter list) but
/// cannot invent anchors, and extra items it sends become unanchored notes.
pub fn merge_edits(stored: &Minutes, data: &Value) -> Minutes {
    let text = |v: Option<&Value>, fallback: &str| -> String {
        match v {
            None | Some(Value::Null) => squash(fallback),
            Some(other) => squash(&match other {
                Value::String(s) => s.clone(),
                Value::Bool(true) => "True".into(),
                Value::Bool(false) => "False".into(),
                Value::Number(n) => n.to_string(),
                o => o.to_string(),
            }),
        }
    };
    let edits = |key: &str| -> Option<Vec<&Map<String, Value>>> {
        data.get(key).map(|v| match v {
            Value::Array(items) => items.iter().filter_map(Value::as_object).collect(),
            _ => vec![],
        })
    };

    let mut out = stored.clone();
    out.title = text(data.get("title"), &out.title);
    out.overview = text(data.get("overview"), &out.overview);
    if data.get("highlights").is_some() {
        out.highlights = match data.get("highlights") {
            Some(Value::Array(h)) => h.iter().map(|x| text(Some(x), "")).filter(|s| !s.is_empty()).collect(),
            _ => vec![],
        };
    }
    if let Some(items) = edits("actions") {
        out.actions = items
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let mut a = stored.actions.get(i).cloned().unwrap_or_else(|| Action::new("", "", "", 0.0, vec![]));
                a.task = text(e.get("task"), &a.task);
                a.owner = text(e.get("owner"), &a.owner);
                a.due = text(e.get("due"), &a.due);
                a.done = e.get("done").map_or(a.done, truthy);
                (!a.task.is_empty()).then_some(a)
            })
            .collect();
    }
    let points = |items: Vec<&Map<String, Value>>, base: &[Point]| -> Vec<Point> {
        items
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let mut p = base.get(i).cloned().unwrap_or_else(|| Point::new("", 0.0, vec![]));
                p.text = text(e.get("text"), &p.text);
                (!p.text.is_empty()).then_some(p)
            })
            .collect()
    };
    if let Some(items) = edits("decisions") {
        out.decisions = points(items, &stored.decisions);
    }
    if let Some(items) = edits("questions") {
        out.questions = points(items, &stored.questions);
    }
    if let Some(items) = edits("chapters") {
        out.chapters = items
            .iter()
            .enumerate()
            .filter_map(|(i, e)| {
                let mut c = stored.chapters.get(i).cloned().unwrap_or(Chapter { title: String::new(), summary: String::new(), start: 0.0, refs: vec![] });
                c.title = text(e.get("title"), &c.title);
                c.summary = text(e.get("summary"), &c.summary);
                (!c.title.is_empty() || !c.summary.is_empty()).then_some(c)
            })
            .collect();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::write_wav;
    use crate::pipeline::tests::Fakes;
    use crate::recorder::RecorderError;
    use crate::transcript::ME;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    /// A recorder that records nothing; ``fail`` makes start() fail like a missing device.
    #[derive(Default)]
    struct FakeRecorder {
        on: AtomicBool,
        fail: bool,
    }

    impl AudioRecorder for FakeRecorder {
        fn start(&self, folder: &Path) -> Result<HashMap<String, String>, RecorderError> {
            std::fs::create_dir_all(folder).unwrap();
            if self.fail {
                return Err(RecorderError("Perangkat audio tidak ditemukan".into()));
            }
            self.on.store(true, Ordering::SeqCst);
            Ok(HashMap::from([("mic".into(), "Mic".into())]))
        }
        fn stop(&self) -> f64 {
            self.on.store(false, Ordering::SeqCst);
            1.0
        }
        fn recording(&self) -> bool {
            self.on.load(Ordering::SeqCst)
        }
        fn elapsed(&self) -> f64 {
            0.0
        }
        fn levels(&self) -> HashMap<String, f32> {
            HashMap::new()
        }
        fn devices(&self) -> HashMap<String, String> {
            HashMap::new()
        }
        fn silent_tracks(&self) -> Vec<String> {
            vec![]
        }
    }

    fn fake_engines() -> EngineFactory {
        Box::new(|_, _| {
            Ok(Box::new(Fakes::saying(&[
                (ME, vec![Utterance::new(ME, 0.0, 1.0, "Halo semua")]),
                ("Pembicara", vec![Utterance::new("Pembicara", 0.0, 1.0, "Impor")]),
            ])) as Box<dyn Engines>)
        })
    }

    fn service_with(dir: &Path, recorder: FakeRecorder, engines: EngineFactory) -> Arc<LariksaService> {
        let paths = Paths::new(dir.to_path_buf()).ensure().unwrap();
        LariksaService::with(paths, Box::new(recorder), engines).unwrap()
    }

    fn svc(dir: &Path) -> Arc<LariksaService> {
        service_with(dir, FakeRecorder::default(), fake_engines())
    }

    fn wait_status(svc: &LariksaService, id: &str, statuses: &[&str]) -> crate::store::Meeting {
        let deadline = Instant::now() + Duration::from_secs(5);
        while Instant::now() < deadline {
            if let Some(m) = svc.store.get(id).filter(|m| statuses.contains(&m.status.as_str())) {
                return m;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("status stuck at {}", svc.store.get(id).unwrap().status);
    }

    fn audio_file(dir: &Path, name: &str) -> PathBuf {
        let src = dir.join(name);
        std::fs::write(&src, vec![b'x'; 4096]).unwrap();
        src
    }

    fn id_of(v: &Value) -> String {
        v["meeting_id"].as_str().unwrap().to_string()
    }

    #[test]
    fn import_runs_through_worker() {
        let dir = tempfile::tempdir().unwrap();
        let svc = svc(dir.path());
        let result = svc.import_file(&audio_file(dir.path(), "rapat.m4a"), "");
        assert_eq!(result["ok"], true);
        let m = wait_status(&svc, &id_of(&result), &[DONE, FAILED]);
        assert_eq!(m.status, DONE);
        assert_eq!(m.transcript[0].text, "Impor");
        let detail = svc.meeting(&m.id);
        assert!(detail["markdown"].as_str().unwrap().contains("Deploy"));
        assert!(detail["whatsapp"].as_str().unwrap().starts_with('*'));
        assert_eq!(detail["lines"][0]["n"], 1);
        assert_eq!(detail["lines"][0]["speaker"], "Pembicara");
        svc.shutdown();
    }

    #[test]
    fn worker_survives_an_engine_setup_error() {
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let c = calls.clone();
        let svc = service_with(dir.path(), FakeRecorder::default(), Box::new(move |_, _| {
            if c.fetch_add(1, Ordering::SeqCst) == 0 {
                anyhow::bail!("GPU driver hilang");
            }
            Ok(Box::new(Fakes::saying(&[("Pembicara", vec![Utterance::new("Pembicara", 0.0, 1.0, "Halo")])])) as Box<dyn Engines>)
        }));
        let src = audio_file(dir.path(), "a.wav");
        let first = id_of(&svc.import_file(&src, ""));
        assert_eq!(wait_status(&svc, &first, &[FAILED]).error, "GPU driver hilang");
        let second = id_of(&svc.import_file(&src, ""));
        assert_eq!(wait_status(&svc, &second, &[DONE]).status, DONE);
        svc.shutdown();
    }

    #[test]
    fn import_rejects_unknown_type() {
        let dir = tempfile::tempdir().unwrap();
        let svc = svc(dir.path());
        let bad = dir.path().join("notes.pdf");
        std::fs::write(&bad, "x").unwrap();
        assert_eq!(svc.import_file(&bad, "")["ok"], false);
        svc.shutdown();
    }

    #[test]
    fn crash_during_recording_is_recovered_on_start() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf()).ensure().unwrap();
        let id = {
            let store = Store::open(&paths.database()).unwrap();
            let m = store.create("", "record", "id", RECORDING, None).unwrap();
            let folder = paths.meeting_dir(&m.id);
            std::fs::create_dir_all(&folder).unwrap();
            write_wav(&folder.join("mic.wav"), 16000, &[1; 16000]).unwrap();
            m.id
        };
        let svc = service_with(dir.path(), FakeRecorder::default(), fake_engines());
        let m = wait_status(&svc, &id, &[DONE, FAILED]);
        assert_eq!((m.status.as_str(), m.duration), (DONE, 1.0)); // length recovered from the repaired file
        svc.shutdown();
    }

    #[test]
    fn retry_summary_only_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let svc = svc(dir.path());
        let id = id_of(&svc.import_file(&audio_file(dir.path(), "a.wav"), ""));
        wait_status(&svc, &id, &[DONE]);
        assert_eq!(svc.retry(&id, true)["ok"], true);
        assert_eq!(wait_status(&svc, &id, &[DONE]).status, DONE);
        assert_eq!(svc.delete(&id)["ok"], true);
        assert!(svc.meeting(&id).is_null());
        assert!(!svc.paths.meeting_dir(&id).exists());
        svc.shutdown();
    }

    #[test]
    fn failed_recording_start_leaves_nothing_behind() {
        let dir = tempfile::tempdir().unwrap();
        let svc = service_with(dir.path(), FakeRecorder { fail: true, ..Default::default() }, fake_engines());
        assert_eq!(svc.start_recording(""), json!({"ok": false, "error": "Perangkat audio tidak ditemukan"}));
        assert_eq!(svc.meetings(""), json!([]));
        assert_eq!(std::fs::read_dir(svc.paths.recordings()).unwrap().count(), 0);
        svc.shutdown();
    }

    #[test]
    fn record_stop_queues_processing() {
        let dir = tempfile::tempdir().unwrap();
        let svc = svc(dir.path());
        let started = svc.start_recording("Tes");
        assert_eq!(started["ok"], true);
        assert_eq!(svc.start_recording("")["error"], "Rekaman sudah berjalan.");
        let id = id_of(&started);
        // The fake recorder writes no files; give it a mic track to transcribe.
        write_wav(&svc.paths.meeting_dir(&id).join(MIC_FILE), 16000, &[1; 16000]).unwrap();
        assert_eq!(svc.stop_recording()["ok"], true);
        assert_eq!(svc.stop_recording()["error"], "Tidak ada rekaman yang berjalan.");
        let m = wait_status(&svc, &id, &[DONE, FAILED]);
        assert_eq!((m.status.as_str(), m.duration), (DONE, 1.0));
        svc.shutdown();
    }

    #[test]
    fn status_reports_missing_assets() {
        let dir = tempfile::tempdir().unwrap();
        let svc = svc(dir.path());
        let s = svc.status();
        assert_eq!(s["ready"], false);
        let needed: Vec<&str> = s["needed"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        assert!(needed.contains(&"whisper-turbo") && needed.contains(&"llm") && needed.contains(&"whisper"));
        assert_eq!(s["queue"], 0);
        assert_eq!(s["recording"], false);
        assert_eq!(s["compute"]["label"], "CPU");
        svc.shutdown();
    }

    #[test]
    fn rename_and_settings() {
        let dir = tempfile::tempdir().unwrap();
        let svc = svc(dir.path());
        let id = id_of(&svc.import_file(&audio_file(dir.path(), "a.wav"), ""));
        assert_eq!(svc.rename(&id, "  ")["ok"], false);
        assert_eq!(svc.rename(&id, "Sprint 12")["ok"], true);
        assert_eq!(svc.store.get(&id).unwrap().title, "Sprint 12");
        let saved = svc.save_settings(json!({"glossary": "Sari, Budi"}).as_object().unwrap()).unwrap();
        assert_eq!(saved["glossary"], json!(["Sari", "Budi"]));
        assert_eq!(svc.store.settings()["notulen_language"], "auto");
        svc.shutdown();
    }

    #[test]
    fn edits_keep_anchors_language_and_can_tick_actions() {
        let dir = tempfile::tempdir().unwrap();
        let svc = svc(dir.path());
        let id = id_of(&svc.import_file(&audio_file(dir.path(), "a.wav"), ""));
        wait_status(&svc, &id, &[DONE]);
        let mut n = svc.store.get(&id).unwrap().notulen.unwrap();
        n.language = "en".into();
        svc.store.update(&id, Update { notulen: Some(Some(n)), ..Default::default() }).unwrap();
        // The UI sends text fields only; it cannot move anchors or change the language.
        let edit = json!({"overview": "Edited", "actions": [
            {"task": "Deploy v2", "owner": "Budi", "due": "Friday", "done": true, "start": 999, "refs": [42]}]});
        assert_eq!(svc.save_notulen(&id, &edit)["ok"], true);
        let got = svc.store.get(&id).unwrap().notulen.unwrap();
        assert_eq!((got.overview.as_str(), got.language.as_str()), ("Edited", "en"));
        let a = &got.actions[0];
        assert_eq!((a.task.as_str(), a.owner.as_str(), a.due.as_str(), a.done, a.refs.clone()), ("Deploy v2", "Budi", "Friday", true, vec![1]));
        let detail = svc.meeting(&id);
        assert_eq!(detail["labels"]["summary"], "Summary");
        assert!(detail["markdown"].as_str().unwrap().contains("## Summary"));
        assert!(detail["markdown"].as_str().unwrap().contains("- [x] **Budi**: Deploy v2"));
        svc.save_notulen(&id, &json!({"actions": []}));
        assert!(svc.store.get(&id).unwrap().notulen.unwrap().actions.is_empty());
        svc.shutdown();
    }

    #[test]
    fn importing_a_readai_transcript_skips_transcription() {
        let dir = tempfile::tempdir().unwrap();
        let svc = svc(dir.path());
        let src = dir.path().join("standup.txt");
        std::fs::write(&src, "0:00 - Budi Santoso\nAku deploy hari Jumat ya.\n\n0:15 - Sari\nOke, aku testing.\n").unwrap();
        let id = id_of(&svc.import_file(&src, ""));
        let m = wait_status(&svc, &id, &[DONE, FAILED]);
        assert_eq!(m.status, DONE, "{}", m.error);
        assert_eq!(m.transcript.iter().map(|u| u.speaker.as_str()).collect::<Vec<_>>(), ["Budi Santoso", "Sari"]);
        assert_eq!(m.duration, 25.0);
        assert_eq!(svc.meeting(&id)["has_audio"], false);
        svc.shutdown();
    }

    #[test]
    fn importing_an_empty_transcript_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let svc = svc(dir.path());
        let src = dir.path().join("empty.vtt");
        std::fs::write(&src, "WEBVTT\n\n").unwrap();
        assert_eq!(svc.import_file(&src, "")["ok"], false);
        assert_eq!(svc.meetings(""), json!([]));
        svc.shutdown();
    }

    #[test]
    fn hotkey_toggles_and_opens_main_window_when_not_ready() {
        use crate::overlay::{toggle_recording, Toggle};
        let dir = tempfile::tempdir().unwrap();
        let svc = svc(dir.path());
        // Models missing: the shortcut opens the main window instead of recording.
        assert_eq!(toggle_recording(&svc), Toggle::ShowMain);
        assert!(!svc.recorder.recording());
        // Everything installed: it starts, then stops.
        for a in crate::catalog::assets() {
            std::fs::create_dir_all(a.path(&svc.paths)).unwrap();
            std::fs::write(a.path(&svc.paths).join(&a.marker), b"x").unwrap();
        }
        assert_eq!(toggle_recording(&svc), Toggle::Started);
        assert!(svc.recorder.recording());
        assert_eq!(toggle_recording(&svc), Toggle::Stopped);
        assert!(!svc.recorder.recording());
        svc.shutdown();
    }

    /// Real WASAPI capture: both tracks follow the wall clock even when nothing plays.
    #[cfg(windows)]
    #[test]
    fn real_two_track_recording_on_windows() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::new(dir.path().to_path_buf()).ensure().unwrap();
        let svc = LariksaService::with(paths, Box::new(crate::recorder::Recorder::default()), fake_engines()).unwrap();
        let started = svc.start_recording("Tes perekam");
        assert_eq!(started["ok"], true, "{started}");
        std::thread::sleep(Duration::from_secs(2));
        assert_eq!(svc.stop_recording()["ok"], true);
        let folder = svc.paths.meeting_dir(&id_of(&started));
        for name in [MIC_FILE, SYSTEM_FILE] {
            let (rate, data) = crate::audio::read_wav(&folder.join(name)).unwrap();
            let secs = data.len() as f64 / rate as f64;
            assert!(rate == 16000 && (1.7..=2.5).contains(&secs), "{name}: {secs}");
        }
        svc.shutdown();
    }

    #[test]
    fn meeting_detail_has_the_shape_the_ui_reads() {
        let dir = tempfile::tempdir().unwrap();
        let svc = svc(dir.path());
        let id = id_of(&svc.import_file(&audio_file(dir.path(), "a.wav"), ""));
        wait_status(&svc, &id, &[DONE]);
        let d = svc.meeting(&id);
        for key in ["id", "title", "created_at", "duration", "status", "source", "preview", "action_count", "language", "error",
                    "transcript", "notulen", "timings", "lines", "has_audio", "audio_url", "labels", "markdown", "whatsapp"] {
            assert!(d.get(key).is_some(), "missing {key}");
        }
        assert!(d["audio_url"].as_str().unwrap().starts_with("http://127.0.0.1:"));
        for key in ["title", "overview", "highlights", "chapters", "actions", "decisions", "questions", "speakers", "language", "version"] {
            assert!(d["notulen"].get(key).is_some(), "notulen missing {key}");
        }
        svc.shutdown();
    }
}
