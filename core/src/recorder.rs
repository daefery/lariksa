//! Two-track meeting recorder for Windows (WASAPI).
//!
//! ``mic.wav`` is the default microphone, ``system.wav`` is a loopback of the
//! default speakers/headset, i.e. everyone else in Meet, Zoom or Teams. Keeping
//! them separate is what gives "Saya" vs "Peserta" labels for free.
//!
//! WASAPI loopback delivers nothing while nothing plays, so a naive recording of
//! a quiet stretch is shorter and every later timestamp drifts. Two guards keep
//! both tracks on the wall clock: a silent output stream keeps the audio engine
//! ticking, and the writer pads silence for any gap that still slips through.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Instant;

use crate::audio::{missing_frames, peak, Downsampler, WavWriter, TARGET_RATE};
pub use crate::pipeline::{MIC_FILE, SYSTEM_FILE};

/// Gaps longer than this (seconds) in a track are filled with silence.
pub const GAP_TOLERANCE: f64 = 0.25;

#[derive(Debug)]
pub struct RecorderError(pub String);

impl std::fmt::Display for RecorderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RecorderError {}

/// Live state of one track, shared between its capture thread and the UI.
#[derive(Default)]
pub struct TrackState {
    level: AtomicU32,
    loudest: AtomicU32,
}

impl TrackState {
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }
    pub fn loudest(&self) -> f32 {
        f32::from_bits(self.loudest.load(Ordering::Relaxed))
    }
    fn set_level(&self, v: f32) {
        self.level.store(v.to_bits(), Ordering::Relaxed);
    }
    fn set_loudest(&self, v: f32) {
        self.loudest.store(v.to_bits(), Ordering::Relaxed);
    }
}

/// Turns captured chunks into a 16 kHz mono WAV that follows the wall clock.
pub struct TrackWriter {
    out: WavWriter,
    resample: Downsampler,
    pub state: Arc<TrackState>,
}

impl TrackWriter {
    pub fn new(path: &Path, rate: u32, channels: usize, state: Arc<TrackState>) -> anyhow::Result<Self> {
        Ok(Self { out: WavWriter::create(path, TARGET_RATE)?, resample: Downsampler::new(rate, channels), state })
    }

    /// One chunk of interleaved samples (int16 scale), or an empty slice when nothing arrived.
    pub fn push(&mut self, elapsed_s: f64, interleaved: &[f32]) -> anyhow::Result<()> {
        let pcm = if interleaved.is_empty() { vec![] } else { self.resample.process(interleaved) };
        self.pad(elapsed_s, pcm.len() as u64)?;
        if pcm.is_empty() {
            self.state.set_level(self.state.level() * 0.85);
        } else {
            self.out.write(&pcm)?;
            let level = peak(&pcm);
            self.state.set_level(level.max(self.state.level() * 0.85));
            self.state.set_loudest(self.state.loudest().max(level));
        }
        Ok(())
    }

    fn pad(&mut self, elapsed_s: f64, incoming: u64) -> anyhow::Result<()> {
        let gap = missing_frames(elapsed_s, TARGET_RATE, self.out.frames(), incoming, GAP_TOLERANCE);
        if gap > 0 {
            self.out.write_silence(gap)?;
        }
        Ok(())
    }

    pub fn finish(mut self, elapsed_s: f64) -> anyhow::Result<()> {
        self.pad(elapsed_s, 0)?;
        self.state.set_level(0.0);
        self.out.finish()
    }
}

/// The recorder the service talks to. Tests swap in a fake through this trait.
pub trait AudioRecorder: Send + Sync {
    fn start(&self, folder: &Path) -> Result<HashMap<String, String>, RecorderError>;
    /// Stop and flush both files. Returns the recorded duration in seconds.
    fn stop(&self) -> f64;
    fn recording(&self) -> bool;
    fn elapsed(&self) -> f64;
    fn levels(&self) -> HashMap<String, f32>;
    fn devices(&self) -> HashMap<String, String>;
    /// Tracks with no sound at all so far, typically a muted mic.
    fn silent_tracks(&self) -> Vec<String>;
}

pub struct Recorder {
    inner: std::sync::Mutex<Option<Session>>,
    running: Arc<AtomicBool>,
    last_devices: std::sync::Mutex<HashMap<String, String>>,
}

struct Session {
    started: Instant,
    tracks: Vec<(String, Arc<TrackState>)>,
    threads: Vec<std::thread::JoinHandle<()>>,
}

impl Default for Recorder {
    fn default() -> Self {
        Self { inner: Default::default(), running: Arc::new(AtomicBool::new(false)), last_devices: Default::default() }
    }
}

impl AudioRecorder for Recorder {
    fn start(&self, folder: &Path) -> Result<HashMap<String, String>, RecorderError> {
        let mut inner = self.inner.lock().unwrap();
        if inner.is_some() {
            return Err(RecorderError("Rekaman sudah berjalan.".into()));
        }
        std::fs::create_dir_all(folder).map_err(|e| RecorderError(e.to_string()))?;
        self.running.store(true, Ordering::SeqCst);
        match wasapi_impl::start(folder, self.running.clone()) {
            Ok((devices, tracks, threads)) => {
                *self.last_devices.lock().unwrap() = devices.clone();
                *inner = Some(Session { started: Instant::now(), tracks, threads });
                log::info!("Recording mic={:?} system={:?}", devices.get("mic"), devices.get("system"));
                Ok(devices)
            }
            Err(e) => {
                self.running.store(false, Ordering::SeqCst);
                Err(e)
            }
        }
    }

    fn stop(&self) -> f64 {
        let Some(session) = self.inner.lock().unwrap().take() else { return 0.0 };
        let duration = session.started.elapsed().as_secs_f64();
        self.running.store(false, Ordering::SeqCst);
        for t in session.threads {
            let _ = t.join();
        }
        duration
    }

    fn recording(&self) -> bool {
        self.inner.lock().unwrap().is_some()
    }

    fn elapsed(&self) -> f64 {
        self.inner.lock().unwrap().as_ref().map_or(0.0, |s| s.started.elapsed().as_secs_f64())
    }

    fn levels(&self) -> HashMap<String, f32> {
        let inner = self.inner.lock().unwrap();
        let mut out: HashMap<String, f32> = HashMap::from([("mic".into(), 0.0), ("system".into(), 0.0)]);
        if let Some(s) = inner.as_ref() {
            for (name, state) in &s.tracks {
                out.insert(name.clone(), (state.level() * 1000.0).round() / 1000.0);
            }
        }
        out
    }

    fn devices(&self) -> HashMap<String, String> {
        self.last_devices.lock().unwrap().clone()
    }

    /// Digital near-zero for several seconds is not a quiet room (a real mic
    /// always has a noise floor); it is a mute key or a disabled device.
    fn silent_tracks(&self) -> Vec<String> {
        let inner = self.inner.lock().unwrap();
        match inner.as_ref() {
            Some(s) if s.started.elapsed().as_secs_f64() >= 8.0 => {
                s.tracks.iter().filter(|(_, t)| t.loudest() < 0.002).map(|(n, _)| n.clone()).collect()
            }
            _ => vec![],
        }
    }
}

#[cfg(not(windows))]
mod wasapi_impl {
    use super::*;
    pub type Started = (HashMap<String, String>, Vec<(String, Arc<TrackState>)>, Vec<std::thread::JoinHandle<()>>);
    pub fn start(_: &Path, _: Arc<AtomicBool>) -> Result<Started, RecorderError> {
        Err(RecorderError("Perekam audio hanya tersedia di Windows.".into()))
    }
}

#[cfg(windows)]
mod wasapi_impl {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::mpsc;
    use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

    pub type Started = (HashMap<String, String>, Vec<(String, Arc<TrackState>)>, Vec<std::thread::JoinHandle<()>>);

    fn err(prefix: &str, e: impl std::fmt::Display) -> RecorderError {
        RecorderError(format!("{prefix}: {e}"))
    }

    /// Each stream runs on its own thread with its own COM apartment.
    pub fn start(folder: &Path, running: Arc<AtomicBool>) -> Result<Started, RecorderError> {
        let started = Instant::now();
        let mut devices = HashMap::new();
        let mut tracks = Vec::new();
        let mut threads = Vec::new();
        for (name, file, direction) in [("mic", MIC_FILE, Direction::Capture), ("system", SYSTEM_FILE, Direction::Render)] {
            let state = Arc::new(TrackState::default());
            let (tx, rx) = mpsc::channel::<Result<String, RecorderError>>();
            let (path, run_flag, st) = (folder.join(file), running.clone(), state.clone());
            let handle = std::thread::Builder::new()
                .name(format!("rec-{name}"))
                .spawn(move || capture(direction, &path, run_flag, st, started, tx))
                .map_err(|e| err("Gagal mulai merekam", e))?;
            let failure = match rx.recv() {
                Ok(Ok(device)) => {
                    devices.insert(name.to_string(), device);
                    tracks.push((name.to_string(), state));
                    threads.push(handle);
                    continue;
                }
                Ok(Err(e)) => e,
                Err(_) => RecorderError("Gagal mulai merekam.".into()),
            };
            // Undo the track that did start, so nothing is left half-open.
            running.store(false, Ordering::SeqCst);
            for t in threads {
                let _ = t.join();
            }
            let _ = handle.join();
            return Err(failure);
        }
        // Keep the audio engine awake so loopback delivers silence instead of nothing.
        let keep = running.clone();
        threads.push(std::thread::spawn(move || keep_engine_awake(keep)));
        Ok((devices, tracks, threads))
    }

    fn capture(
        direction: Direction,
        path: &Path,
        running: Arc<AtomicBool>,
        state: Arc<TrackState>,
        started: Instant,
        ready: mpsc::Sender<Result<String, RecorderError>>,
    ) {
        let _ = wasapi::initialize_mta();
        let setup = || -> Result<_, RecorderError> {
            let enumerator = DeviceEnumerator::new().map_err(|e| err("Perangkat audio tidak ditemukan", e))?;
            let device = enumerator.get_default_device(&direction).map_err(|e| err("Perangkat audio tidak ditemukan", e))?;
            let name = device.get_friendlyname().unwrap_or_default();
            let mut client = device.get_iaudioclient().map_err(|e| err("Perangkat audio tidak ditemukan", e))?;
            let mix = client.get_mixformat().map_err(|e| err("Format audio", e))?;
            let rate = mix.get_samplespersec();
            let channels = mix.get_nchannels().clamp(1, 2) as usize;
            // Let Windows convert to float at the device rate; the writer resamples to 16 kHz.
            let format = WaveFormat::new(32, 32, &SampleType::Float, rate as usize, channels, None);
            let (_, min_period) = client.get_device_period().map_err(|e| err("Gagal mulai merekam", e))?;
            let mode = StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: min_period.max(200_000) };
            // A render device opened for capture is WASAPI loopback.
            client.initialize_client(&format, &Direction::Capture, &mode).map_err(|e| err("Gagal mulai merekam", e))?;
            let event = client.set_get_eventhandle().map_err(|e| err("Gagal mulai merekam", e))?;
            let capture = client.get_audiocaptureclient().map_err(|e| err("Gagal mulai merekam", e))?;
            let writer = TrackWriter::new(path, rate, channels, state.clone()).map_err(|e| err("Gagal membuat file", e))?;
            client.start_stream().map_err(|e| err("Gagal mulai merekam", e))?;
            Ok((name, client, event, capture, writer))
        };
        let (name, client, event, capture, mut writer) = match setup() {
            Ok(v) => v,
            Err(e) => {
                let _ = ready.send(Err(e));
                return;
            }
        };
        let _ = ready.send(Ok(name));

        let mut queue: VecDeque<u8> = VecDeque::new();
        let mut samples: Vec<f32> = Vec::new();
        // One wake can cover several packets; read until the device has none left.
        let drain = |queue: &mut VecDeque<u8>| -> bool {
            loop {
                match capture.get_next_packet_size() {
                    Ok(Some(0)) => return true,
                    Ok(_) => {
                        if capture.read_from_device_to_deque(queue).is_err() {
                            return false;
                        }
                    }
                    Err(_) => return false,
                }
            }
        };
        let mut device_ok = true;
        loop {
            let stopping = !running.load(Ordering::SeqCst);
            if !stopping {
                // Loopback signals no event while nothing plays: time out, then pad.
                let _ = event.wait_for_event(250);
            }
            if !drain(&mut queue) {
                log::warn!("Audio device lost while recording");
                device_ok = false;
            }
            samples.clear();
            samples.extend(queue.drain(..queue.len() - queue.len() % 4).collect::<Vec<u8>>().chunks_exact(4).map(|b| {
                f32::from_le_bytes([b[0], b[1], b[2], b[3]]) * 32767.0
            }));
            if let Err(e) = writer.push(started.elapsed().as_secs_f64(), &samples) {
                log::error!("Writing audio failed: {e}");
                break;
            }
            if stopping || !device_ok {
                break; // the last pass above flushed what the device still held
            }
        }
        let _ = client.stop_stream();
        if let Err(e) = writer.finish(started.elapsed().as_secs_f64()) {
            log::error!("Closing audio file failed: {e}");
        }
    }

    /// A silent output stream keeps the shared audio engine ticking.
    fn keep_engine_awake(running: Arc<AtomicBool>) {
        let _ = wasapi::initialize_mta();
        let run = || -> Result<(), Box<dyn std::error::Error>> {
            let device = DeviceEnumerator::new()?.get_default_device(&Direction::Render)?;
            let mut client = device.get_iaudioclient()?;
            let mix = client.get_mixformat()?;
            let format = WaveFormat::new(32, 32, &SampleType::Float, mix.get_samplespersec() as usize, mix.get_nchannels() as usize, None);
            let (_, min_period) = client.get_device_period()?;
            client.initialize_client(&format, &Direction::Render, &StreamMode::EventsShared { autoconvert: true, buffer_duration_hns: min_period.max(200_000) })?;
            let event = client.set_get_eventhandle()?;
            let render = client.get_audiorenderclient()?;
            let block = format.get_blockalign() as usize;
            client.start_stream()?;
            while running.load(Ordering::SeqCst) {
                let frames = client.get_available_space_in_frames()? as usize;
                if frames > 0 {
                    render.write_to_device(frames, &vec![0u8; frames * block], None)?;
                }
                let _ = event.wait_for_event(250);
            }
            client.stop_stream()?;
            Ok(())
        };
        if let Err(e) = run() {
            log::warn!("Silent keep-alive stream unavailable: {e}"); // padding in the writer still covers gaps
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::read_wav;

    #[test]
    fn writer_pads_gaps_and_tracks_levels() {
        let dir = tempfile::tempdir().unwrap();
        let state = Arc::new(TrackState::default());
        let path = dir.path().join("system.wav");
        let mut w = TrackWriter::new(&path, 48_000, 2, state.clone()).unwrap();
        // 0.5 s of sound, then nothing arrives for 1.5 s (loopback while nothing plays).
        // A 440 Hz tone, the same on both channels.
        let chunk: Vec<f32> = (0..24_000).flat_map(|i| {
            let v = (2.0 * std::f32::consts::PI * 440.0 * i as f32 / 48_000.0).sin() * 8000.0;
            [v, v]
        }).collect();
        w.push(0.5, &chunk).unwrap();
        assert!(state.level() > 0.2 && state.loudest() > 0.2);
        w.push(1.0, &[]).unwrap();
        w.finish(2.0).unwrap();
        let (rate, data) = read_wav(&path).unwrap();
        assert_eq!(rate, 16_000);
        assert!((data.len() as i64 - 32_000).abs() <= 2, "{}", data.len()); // the wall clock, not the sound
    }
}
