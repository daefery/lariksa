//! `retranscribe <portable-root> <meeting-folder> [glossary,terms]`: transcribe a recorded
//! meeting the way the app does (both tracks, echo removal, merge by time) and print the
//! transcript. Read-only: the database is not touched.
use lariksa_core::{catalog, hardware, paths::Paths, pipeline::tracks_for, transcribe::*, transcript::*};
use std::path::Path;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let paths = Paths::new(a[1].clone().into());
    let folder = Path::new(&a[2]);
    let glossary: Vec<String> = a.get(3).map(|g| g.split(',').map(String::from).collect()).unwrap_or_default();
    let server = catalog::asset(catalog::LLAMA_RUNTIME).unwrap().path(&paths).join(catalog::exe("llama-server"));
    let gpu = hardware::choose_device(&hardware::list_devices(&server), hardware::WHISPER_NEED_MIB).and_then(|d| d.index());
    let w = catalog::asset(catalog::WHISPER_TURBO).unwrap();
    let v = catalog::asset(catalog::VAD_MODEL).unwrap();
    let t = Transcriber::new(TranscribeOptions {
        cli: catalog::asset(catalog::WHISPER_RUNTIME).unwrap().path(&paths).join(catalog::exe("whisper-cli")),
        model: w.path(&paths).join(&w.marker),
        vad: v.path(&paths).join(&v.marker),
        gpu,
        language: String::new(),
        glossary,
    })
    .unwrap();
    let (mut mine, mut theirs) = (vec![], vec![]);
    for (path, speaker) in tracks_for(folder) {
        let u = t.transcribe(&path, speaker, &mut |_| {}, &|| false).unwrap();
        if speaker == ME { mine = u } else { theirs = u }
    }
    for u in merge_tracks(mine, theirs) {
        println!("{:>7.1}-{:>7.1} {:<8} {}", u.start, u.end, u.speaker, u.text);
    }
}
