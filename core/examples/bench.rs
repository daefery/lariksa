//! `bench <portable-root> <audio.wav>`: time whisper.cpp on the chosen GPU and on CPU, as the app runs it.
use lariksa_core::{catalog, hardware, paths::Paths, transcribe::*};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let paths = Paths::new(a[1].clone().into());
    let server = catalog::asset(catalog::LLAMA_RUNTIME).unwrap().path(&paths).join(catalog::exe("llama-server"));
    let gpu = hardware::choose_device(&hardware::list_devices(&server), hardware::WHISPER_NEED_MIB).and_then(|d| d.index());
    let w = catalog::asset(catalog::WHISPER_TURBO).unwrap();
    let v = catalog::asset(catalog::VAD_MODEL).unwrap();
    for (label, dev) in [("GPU", gpu), ("CPU", None)] {
        let o = TranscribeOptions {
            cli: catalog::asset(catalog::WHISPER_RUNTIME).unwrap().path(&paths).join(catalog::exe("whisper-cli")),
            model: w.path(&paths).join(&w.marker), vad: v.path(&paths).join(&v.marker),
            gpu: dev, language: "id".into(), glossary: vec![],
        };
        let t = std::time::Instant::now();
        let u = Transcriber::new(o).unwrap().transcribe(std::path::Path::new(&a[2]), "Peserta", &mut |_| {}, &|| false).unwrap();
        println!("{label}: {:.1}s, {} utterances", t.elapsed().as_secs_f64(), u.len());
    }
}
