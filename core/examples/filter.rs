//! `filter <whisper-cli.json>...`: which raw segments the hallucination filter drops.
use lariksa_core::transcribe::{build_prompt, drop_hallucinations, parse_output};
fn main() {
    let prompt = build_prompt("id", &[]);
    for f in std::env::args().skip(1) {
        let json: serde_json::Value = serde_json::from_slice(&std::fs::read(&f).unwrap()).unwrap();
        let all = parse_output(&json);
        let kept = drop_hallucinations(all.clone(), &prompt);
        println!("{f}: kept {}/{}", kept.len(), all.len());
        for s in all.iter().filter(|s| !kept.contains(s)) {
            println!("   dropped [{:.0}-{:.0}] {}", s.start, s.end, s.text.chars().take(80).collect::<String>());
        }
    }
}
