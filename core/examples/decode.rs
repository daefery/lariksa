fn main() {
    for p in std::env::args().skip(1) {
        let out = std::path::PathBuf::from(&p).with_extension("dec.wav");
        match lariksa_core::audio::decode_to_wav(std::path::Path::new(&p), &out) {
            Ok(s) => println!("{p}: {s:.2}s"),
            Err(e) => println!("{p}: ERROR {e}"),
        }
    }
}
