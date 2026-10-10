//! Windows smoke test for the native pieces: `probe <portable-root> [audio.wav]`.
//! Records 3 s of both tracks, lists GPUs, and transcribes a file with whisper.cpp.
use lariksa_core::{catalog, hardware, paths::Paths, recorder::{AudioRecorder, Recorder}, transcribe::*};
use std::time::Instant;

fn main() {
    let root = std::env::args().nth(1).expect("root");
    let paths = Paths::new(root.into());
    let rec = Recorder::default();
    let folder = paths.root.join("probe-rec");
    match rec.start(&folder) {
        Ok(d) => {
            println!("recording devices: {d:?}");
            std::thread::sleep(std::time::Duration::from_secs(3));
            println!("levels {:?}", rec.levels());
            let secs = rec.stop();
            for n in ["mic.wav", "system.wav"] {
                let (rate, s) = lariksa_core::audio::read_wav(&folder.join(n)).unwrap();
                println!("{n}: {rate} Hz, {:.2} s (wall {secs:.2})", s.len() as f64 / rate as f64);
            }
        }
        Err(e) => println!("record error: {e}"),
    }
    let server = catalog::asset(catalog::LLAMA_RUNTIME).unwrap().path(&paths).join("llama-server.exe");
    let devices = hardware::list_devices(&server);
    println!("devices: {devices:?}");
    let gpu = hardware::choose_device(&devices, hardware::WHISPER_NEED_MIB);
    println!("chosen: {}", hardware::label(gpu));
    if let Some(audio) = std::env::args().nth(2) {
        let wcpp = std::env::args().nth(3).expect("whisper dir");
        let o = TranscribeOptions {
            cli: std::path::Path::new(&wcpp).join("whisper-cli.exe"),
            model: std::path::Path::new(&wcpp).join(catalog::TURBO_FILE),
            vad: std::path::Path::new(&wcpp).join(catalog::VAD_FILE),
            gpu: gpu.and_then(|d| d.index()),
            language: "id".into(),
            glossary: vec!["Lariksa".into()],
        };
        let t = Transcriber::new(o).unwrap();
        let start = Instant::now();
        let mut last = -1.0;
        let u = t.transcribe(std::path::Path::new(&audio), "Peserta", &mut |p| { if p - last >= 0.25 { println!("progress {p:.2}"); last = p; } }, &|| false).unwrap();
        println!("{} utterances in {:.1}s", u.len(), start.elapsed().as_secs_f64());
        let out: Vec<_> = u.iter().map(|x| serde_json::json!({"start": x.start, "end": x.end, "text": x.text})).collect();
        std::fs::write(paths.root.join("probe-asr.json"), serde_json::to_string_pretty(&out).unwrap()).unwrap();
        for x in u.iter().take(5) { println!("[{:.1}-{:.1}] {}", x.start, x.end, x.text); }
    }
}
