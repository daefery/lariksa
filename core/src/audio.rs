//! Small, pure audio helpers: resampling, levels, WAV files, decoding imports.
//!
//! Tracks are stored as 16 kHz mono 16-bit WAV, the format Whisper reads
//! natively: about 115 MB per hour per track instead of ~700 MB for 48 kHz
//! stereo, and no resampling step before transcription.

use anyhow::{anyhow, bail, Context, Result};
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

pub const TARGET_RATE: u32 = 16_000;

/// Streaming PCM -> 16 kHz mono int16, continuous across chunks.
///
/// Mixes channels down, applies a short moving-average low-pass sized to the
/// rate ratio (cheap anti-aliasing that is plenty for speech), then linearly
/// interpolates at the target rate. The read position and the filter history
/// carry over between chunks, so chunk boundaries leave no clicks or drift.
pub struct Downsampler {
    channels: usize,
    step: f64,
    taps: usize,
    history: Vec<f32>,
    pos: f64,
}

impl Downsampler {
    pub fn new(rate: u32, channels: usize) -> Self {
        let step = rate as f64 / TARGET_RATE as f64;
        let taps = (step.round() as usize).max(1);
        // Positions index `signal`, where 0 is the previous chunk's last sample;
        // start at 1, the first new sample, so there is no added delay.
        Self { channels: channels.max(1), step, taps, history: vec![0.0; taps], pos: 1.0 }
    }

    /// Interleaved samples (any scale) in, 16 kHz mono int16 out.
    pub fn process(&mut self, interleaved: &[f32]) -> Vec<i16> {
        let usable = interleaved.len() - interleaved.len() % self.channels;
        if usable == 0 {
            return vec![];
        }
        let mono: Vec<f32> = interleaved[..usable]
            .chunks(self.channels)
            .map(|f| f.iter().sum::<f32>() / self.channels as f32)
            .collect();

        let mut joined = self.history.clone();
        joined.extend_from_slice(&mono);
        let smooth: Vec<f32> = if self.taps > 1 {
            let k = self.taps;
            let mut acc: f32 = joined[..k].iter().sum();
            let mut out = Vec::with_capacity(joined.len() - k + 1);
            out.push(acc / k as f32);
            for i in k..joined.len() {
                acc += joined[i] - joined[i - k];
                out.push(acc / k as f32);
            }
            out
        } else {
            joined.clone()
        };
        let signal: Vec<f32> = if smooth.len() > mono.len() {
            smooth[smooth.len() - (mono.len() + 1)..].to_vec()
        } else {
            std::iter::once(0.0).chain(smooth.iter().copied()).collect()
        };

        let last = (signal.len() - 1) as f64;
        let mut out = Vec::new();
        let mut p = self.pos;
        while p < last {
            let i = p.floor() as usize;
            let frac = (p - i as f64) as f32;
            let v = signal[i] + (signal[(i + 1).min(signal.len() - 1)] - signal[i]) * frac;
            out.push(v.round().clamp(-32768.0, 32767.0) as i16);
            p += self.step;
        }
        self.pos = p - last;
        self.history = joined[joined.len() - self.taps..].to_vec();
        out
    }
}

/// Peak level 0..1 of int16 PCM.
pub fn peak(samples: &[i16]) -> f32 {
    samples.iter().map(|s| (*s as i32).abs()).max().map_or(0.0, |m| (m as f32 / 32768.0).min(1.0))
}

/// Frames of silence to insert so a track keeps up with the wall clock.
///
/// WASAPI loopback delivers nothing while nothing plays; without padding,
/// every timestamp after a quiet stretch would be early.
pub fn missing_frames(elapsed_s: f64, rate: u32, written: u64, incoming: u64, tolerance_s: f64) -> u64 {
    let gap = (elapsed_s * rate as f64) as i64 - (written + incoming) as i64;
    if gap as f64 > rate as f64 * tolerance_s { gap as u64 } else { 0 }
}

// ---------------------------------------------------------------------------
// WAV files: canonical 44-byte header, 16-bit mono
// ---------------------------------------------------------------------------

fn header(rate: u32, data_bytes: u32) -> [u8; 44] {
    let mut h = [0u8; 44];
    h[..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data_bytes).to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&1u16.to_le_bytes()); // mono
    h[24..28].copy_from_slice(&rate.to_le_bytes());
    h[28..32].copy_from_slice(&(rate * 2).to_le_bytes());
    h[32..34].copy_from_slice(&2u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_bytes.to_le_bytes());
    h
}

/// A WAV being recorded. The header is fixed up on ``finish``; after a crash,
/// ``repair_wav`` does the same from the file size.
pub struct WavWriter {
    out: BufWriter<File>,
    rate: u32,
    frames: u64,
}

impl WavWriter {
    pub fn create(path: &Path, rate: u32) -> Result<Self> {
        let mut out = BufWriter::new(File::create(path)?);
        out.write_all(&header(rate, 0))?;
        Ok(Self { out, rate, frames: 0 })
    }

    pub fn write(&mut self, samples: &[i16]) -> Result<()> {
        for s in samples {
            self.out.write_all(&s.to_le_bytes())?;
        }
        self.frames += samples.len() as u64;
        Ok(())
    }

    pub fn write_silence(&mut self, frames: u64) -> Result<()> {
        self.out.write_all(&vec![0u8; frames as usize * 2])?;
        self.frames += frames;
        Ok(())
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn finish(mut self) -> Result<()> {
        self.out.flush()?;
        let mut f = self.out.into_inner().map_err(|e| anyhow!("{e}"))?;
        f.seek(SeekFrom::Start(0))?;
        f.write_all(&header(self.rate, (self.frames * 2) as u32))?;
        Ok(())
    }
}

pub fn write_wav(path: &Path, rate: u32, samples: &[i16]) -> Result<()> {
    let mut w = WavWriter::create(path, rate)?;
    w.write(samples)?;
    w.finish()
}

/// Read a 16-bit mono/stereo PCM WAV (any header layout), downmixed to mono.
pub fn read_wav(path: &Path) -> Result<(u32, Vec<i16>)> {
    let mut bytes = Vec::new();
    File::open(path)?.read_to_end(&mut bytes)?;
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        bail!("bukan file WAV");
    }
    let (mut rate, mut channels, mut bits) = (0u32, 0u16, 0u16);
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body = pos + 8;
        if id == b"fmt " && body + 16 <= bytes.len() {
            channels = u16::from_le_bytes([bytes[body + 2], bytes[body + 3]]);
            rate = u32::from_le_bytes(bytes[body + 4..body + 8].try_into().unwrap());
            bits = u16::from_le_bytes([bytes[body + 14], bytes[body + 15]]);
        } else if id == b"data" {
            if bits != 16 || channels == 0 {
                bail!("WAV harus 16-bit PCM");
            }
            let end = (body + size).min(bytes.len());
            let samples: Vec<i16> = bytes[body..end].chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
            let mono = if channels == 1 {
                samples
            } else {
                samples.chunks(channels as usize).map(|f| (f.iter().map(|s| *s as i32).sum::<i32>() / f.len() as i32) as i16).collect()
            };
            return Ok((rate, mono));
        }
        pos = body + size + (size & 1);
    }
    bail!("WAV tanpa data audio")
}

/// Fix the size fields of a canonical 44-byte-header WAV left by a crash.
///
/// The header claims no audio until the recorder closes the file, so a
/// recording interrupted by a crash or power loss looks empty. The audio itself
/// is intact; patching two integers makes it readable again.
pub fn repair_wav(path: &Path) -> Result<bool> {
    let size = std::fs::metadata(path)?.len();
    if size <= 44 {
        return Ok(false);
    }
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
    let mut h = [0u8; 44];
    f.read_exact(&mut h)?;
    if &h[..4] != b"RIFF" || &h[8..12] != b"WAVE" || &h[36..40] != b"data" {
        return Ok(false);
    }
    let riff = u32::from_le_bytes(h[4..8].try_into().unwrap()) as u64;
    let data = u32::from_le_bytes(h[40..44].try_into().unwrap()) as u64;
    if riff == size - 8 && data == size - 44 {
        return Ok(false);
    }
    f.seek(SeekFrom::Start(4))?;
    f.write_all(&((size - 8) as u32).to_le_bytes())?;
    f.seek(SeekFrom::Start(40))?;
    f.write_all(&((size - 44) as u32).to_le_bytes())?;
    Ok(true)
}

/// Sum 16 kHz mono int16 WAVs into one file, sample-aligned from t=0.
///
/// Sum, not average: two tracks rarely peak together, and averaging halves the
/// loudness of whoever is talking. Clipping guards the rare overlap.
pub fn mix_tracks(tracks: &[std::path::PathBuf], target: &Path) -> Result<()> {
    let mut rate = TARGET_RATE;
    let mut data = Vec::new();
    for t in tracks {
        let (r, samples) = read_wav(t).with_context(|| t.display().to_string())?;
        rate = r;
        data.push(samples);
    }
    let length = data.iter().map(Vec::len).max().unwrap_or(0);
    let mut mixed = vec![0i32; length];
    for d in &data {
        for (m, s) in mixed.iter_mut().zip(d) {
            *m += *s as i32;
        }
    }
    let mixed: Vec<i16> = mixed.into_iter().map(|v| v.clamp(-32768, 32767) as i16).collect();
    let tmp = target.with_extension("tmp");
    write_wav(&tmp, rate, &mixed)?;
    std::fs::rename(&tmp, target)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Decoding imported audio and video
// ---------------------------------------------------------------------------

/// Decode any supported audio/video file to 16 kHz mono WAV for Whisper.
///
/// The pure-Rust decoders cover MP3, AAC, ALAC, FLAC, Vorbis, Opus and PCM in
/// MP4/M4A, MKV/WebM, OGG, WAV, AIFF and CAF. Anything else (AC-3, WMA, ...)
/// falls back to Windows' own Media Foundation decoders, as broad as the
/// FFmpeg the earlier version used for typical meeting recordings.
pub fn decode_to_wav(source: &Path, target: &Path) -> Result<f64> {
    match decode_symphonia(source, target) {
        Ok(secs) => Ok(secs),
        #[cfg(windows)]
        Err(first) => media_foundation::decode(source, target).map_err(|e| {
            log::warn!("Decoding {} failed: {first:#}; Media Foundation: {e:#}", source.display());
            first
        }),
        #[cfg(not(windows))]
        Err(e) => Err(e),
    }
}

fn decode_symphonia(source: &Path, target: &Path) -> Result<f64> {
    use symphonia::core::codecs::audio::AudioDecoderOptions;
    use symphonia::core::codecs::registry::CodecRegistry;
    use symphonia::core::errors::Error;
    use symphonia::core::formats::probe::Hint;
    use symphonia::core::formats::{FormatOptions, TrackType};
    use symphonia::core::io::MediaSourceStream;
    use symphonia::core::meta::MetadataOptions;

    let mut registry = CodecRegistry::new();
    symphonia::default::register_enabled_codecs(&mut registry);
    registry.register_audio_decoder::<symphonia_adapter_opus_rs::OpusAdapter>();

    let file = File::open(source)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = source.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(&hint, mss, FormatOptions::default(), MetadataOptions::default())
        .map_err(|e| anyhow!("Format audio tidak dikenali: {e}"))?;
    let track = format.default_track(TrackType::Audio).ok_or_else(|| anyhow!("File tidak berisi audio."))?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| anyhow!("Codec audio tidak dikenali."))?
        .clone();
    let track_id = track.id;
    let mut decoder = registry
        .make_audio_decoder(&params, &AudioDecoderOptions::default())
        .map_err(|e| anyhow!("Codec audio tidak didukung: {e}"))?;

    let mut writer = WavWriter::create(target, TARGET_RATE)?;
    let mut resampler: Option<(u32, usize, Downsampler)> = None;
    let mut interleaved: Vec<f32> = Vec::new();
    loop {
        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => break,
            Err(Error::ResetRequired) => break,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(anyhow!("Gagal membaca audio: {e}")),
        };
        if packet.track_id != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(Error::DecodeError(_)) | Err(Error::IoError(_)) => continue,
            Err(e) => return Err(anyhow!("Gagal mendekode audio: {e}")),
        };
        let rate = decoded.spec().rate();
        let channels = decoded.spec().channels().count().max(1);
        interleaved.clear();
        decoded.copy_to_vec_interleaved(&mut interleaved);
        // f32 samples are -1..1; scale to the int16 range the resampler emits.
        for s in interleaved.iter_mut() {
            *s *= 32767.0;
        }
        let r = match &mut resampler {
            Some((r, c, d)) if *r == rate && *c == channels => d,
            _ => {
                resampler = Some((rate, channels, Downsampler::new(rate, channels)));
                &mut resampler.as_mut().unwrap().2
            }
        };
        writer.write(&r.process(&interleaved))?;
    }
    let frames = writer.frames();
    writer.finish()?;
    if frames == 0 {
        bail!("File tidak berisi audio yang bisa dibaca.");
    }
    Ok(frames as f64 / TARGET_RATE as f64)
}

#[cfg(windows)]
mod media_foundation {
    use super::*;
    use windows::Win32::Media::MediaFoundation::*;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

    /// Windows' decoders, asked for 32-bit float PCM at the source rate.
    pub fn decode(source: &Path, target: &Path) -> Result<f64> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            MFStartup(MF_VERSION, MFSTARTUP_FULL)?;
            let result = run(source, target);
            let _ = MFShutdown();
            result
        }
    }

    unsafe fn run(source: &Path, target: &Path) -> Result<f64> {
        let url: Vec<u16> = source.to_string_lossy().encode_utf16().chain([0]).collect();
        let reader = MFCreateSourceReaderFromURL(windows::core::PCWSTR(url.as_ptr()), None)?;
        let audio = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
        reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)?;
        reader.SetStreamSelection(audio, true)?;
        let wanted = MFCreateMediaType()?;
        wanted.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        wanted.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_Float)?;
        reader.SetCurrentMediaType(audio, None, &wanted)?;
        // Decoders may settle the output layout (e.g. AC-3 decoded to 5.1) only once
        // decoding starts, so the format is read again whenever it changes.
        let format = || -> Result<(u32, usize)> {
            let t = reader.GetCurrentMediaType(audio)?;
            Ok((t.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND)?, t.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS)?.max(1) as usize))
        };
        let (mut rate, mut channels) = format()?;

        let mut writer = WavWriter::create(target, TARGET_RATE)?;
        let mut resampler = Downsampler::new(rate, channels);
        loop {
            let mut flags = 0u32;
            let mut sample: Option<IMFSample> = None;
            reader.ReadSample(audio, 0, None, Some(&mut flags), None, Some(&mut sample))?;
            if flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0 || writer.frames() == 0 {
                let now = format()?;
                if now != (rate, channels) {
                    (rate, channels) = now;
                    resampler = Downsampler::new(rate, channels);
                }
            }
            if let Some(sample) = sample {
                let buffer = sample.ConvertToContiguousBuffer()?;
                let mut ptr = std::ptr::null_mut();
                let mut len = 0u32;
                buffer.Lock(&mut ptr, None, Some(&mut len))?;
                let floats = std::slice::from_raw_parts(ptr as *const f32, len as usize / 4);
                let scaled: Vec<f32> = floats.iter().map(|s| s * 32767.0).collect();
                buffer.Unlock()?;
                writer.write(&resampler.process(&scaled))?;
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                break;
            }
        }
        let frames = writer.frames();
        writer.finish()?;
        if frames == 0 {
            bail!("File tidak berisi audio yang bisa dibaca.");
        }
        Ok(frames as f64 / TARGET_RATE as f64)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(rate: u32, seconds: f64, freq: f64, channels: usize) -> Vec<f32> {
        let n = (rate as f64 * seconds) as usize;
        (0..n)
            .flat_map(|i| {
                let v = ((2.0 * std::f64::consts::PI * freq * i as f64 / rate as f64).sin() * 12000.0) as i16 as f32;
                std::iter::repeat_n(v, channels)
            })
            .collect()
    }

    #[test]
    fn downsample_48k_stereo_to_16k_mono_keeps_duration() {
        let out = Downsampler::new(48_000, 2).process(&sine(48_000, 1.0, 440.0, 2));
        assert!((out.len() as i64 - TARGET_RATE as i64).abs() <= 2);
    }

    #[test]
    fn chunked_output_matches_one_shot() {
        let data = sine(44_100, 2.0, 440.0, 2);
        let one = Downsampler::new(44_100, 2).process(&data);
        let mut ds = Downsampler::new(44_100, 2);
        let many: Vec<i16> = data.chunks(1024 * 2).flat_map(|p| ds.process(p)).collect();
        assert!((one.len() as i64 - many.len() as i64).abs() <= 1);
        let n = one.len().min(many.len());
        assert!(one[..n].iter().zip(&many[..n]).all(|(a, b)| (*a as i32 - *b as i32).abs() <= 1));
    }

    #[test]
    fn tone_survives_downsampling() {
        let out = Downsampler::new(48_000, 1).process(&sine(48_000, 1.0, 440.0, 1));
        // Zero crossings of a 440 Hz tone over one second: ~880.
        let crossings = out.windows(2).filter(|w| (w[0] < 0) != (w[1] < 0)).count();
        assert!((crossings as i64 - 880).abs() < 6, "{crossings}");
    }

    #[test]
    fn mono_16k_passes_through() {
        let data = sine(16_000, 0.5, 440.0, 1);
        let out = Downsampler::new(16_000, 1).process(&data);
        assert!(out[1..100].iter().zip(&data[1..100]).all(|(a, b)| *a as f32 == *b));
    }

    #[test]
    fn peak_level() {
        assert_eq!(peak(&[]), 0.0);
        assert_eq!(peak(&[0, 16384, -32768]), 1.0);
    }

    #[test]
    fn missing_frames_pads_only_real_gaps() {
        assert_eq!(missing_frames(10.0, 16_000, 160_000, 0, 0.25), 0);
        assert_eq!(missing_frames(10.0, 16_000, 150_000, 0, 0.25), 10_000);
        assert_eq!(missing_frames(10.0, 16_000, 158_000, 0, 0.25), 0);
    }

    #[test]
    fn repair_wav_after_crash() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mic.wav");
        write_wav(&path, 16_000, &[0]).unwrap(); // header says 1 frame
        let mut f = std::fs::OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(&[1, 0].repeat(15_999)).unwrap(); // frames written after the header, then a crash
        drop(f);
        assert!(repair_wav(&path).unwrap());
        assert_eq!(read_wav(&path).unwrap().1.len(), 16_000);
        assert!(!repair_wav(&path).unwrap());
    }

    #[test]
    fn mix_aligns_and_clips() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b, m) = (dir.path().join("a.wav"), dir.path().join("b.wav"), dir.path().join("m.wav"));
        write_wav(&a, 16_000, &[1000, 30000, 0]).unwrap();
        write_wav(&b, 16_000, &[500, 10000]).unwrap();
        mix_tracks(&[a, b], &m).unwrap();
        assert_eq!(read_wav(&m).unwrap().1, [1500, 32767, 0]);
    }

    #[test]
    fn decode_wav_import_to_16k() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("in.wav");
        let samples: Vec<i16> = sine(48_000, 1.0, 440.0, 1).into_iter().map(|v| v as i16).collect();
        write_wav(&src, 48_000, &samples).unwrap();
        let out = dir.path().join("out.wav");
        let secs = decode_to_wav(&src, &out).unwrap();
        assert!((secs - 1.0).abs() < 0.01, "{secs}");
        let (rate, data) = read_wav(&out).unwrap();
        assert_eq!(rate, 16_000);
        assert!(peak(&data) > 0.3);
    }
}
