//! Write minutes from a transcript exactly as the app does, for comparing engines:
//!
//!   api_minutes api <base-url> <model> <transcript.json> [language]   key from LARIKSA_API_KEY
//!   api_minutes local <portable-root> <transcript.json> [language]    llama.cpp on the app's GPU
//!
//! The transcript is a JSON list of {speaker, start, end, text}. Prints the
//! seconds taken, then the minutes as JSON.
use lariksa_core::{catalog, hardware, llm::{ApiLlm, LocalLlm}, minutes::lines_from, minutes_writer::write_minutes, paths::Paths, transcript::Utterance};
use serde_json::Value;

fn load(path: &str) -> Vec<Utterance> {
    let raw: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    raw.as_array()
        .unwrap()
        .iter()
        .map(|u| Utterance::new(u["speaker"].as_str().unwrap(), u["start"].as_f64().unwrap(), u["end"].as_f64().unwrap(), u["text"].as_str().unwrap()))
        .collect()
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let t = std::time::Instant::now();
    let minutes = if a[1] == "local" {
        let paths = Paths::new(a[2].clone().into());
        let server = catalog::asset(catalog::LLAMA_RUNTIME).unwrap().path(&paths).join(catalog::exe("llama-server"));
        let model = catalog::asset(catalog::SUMMARY_MODEL).unwrap();
        let devices = hardware::list_devices(&server);
        let device = hardware::choose_device(&devices, hardware::LLM_NEED_MIB).cloned();
        let llm = LocalLlm::start(&server, &model.path(&paths).join(&model.marker), device.as_ref()).unwrap();
        let lines = lines_from(&load(&a[3]));
        write_minutes(&lines, &mut |s, u, k| llm.complete_json(s, u, k), a.get(4).map_or("id", String::as_str), &mut |_| {})
    } else {
        let llm = ApiLlm { url: a[2].clone(), key: std::env::var("LARIKSA_API_KEY").unwrap_or_default(), model: a[3].clone() };
        let lines = lines_from(&load(&a[4]));
        // Raw answers go to stderr, to see what the guardrails kept or dropped.
        let mut traced = |s: &str, u: &str, k| {
            let started = std::time::Instant::now();
            let answer = llm.complete_json(s, u, k);
            eprintln!("{k:?} {:.1}s {}", started.elapsed().as_secs_f64(), answer.as_ref().map(|v| v.to_string()).unwrap_or_else(|e| e.to_string()));
            answer
        };
        write_minutes(&lines, &mut traced, a.get(5).map_or("id", String::as_str), &mut |_| {})
    }
    .unwrap();
    println!("{:.1}\n{}", t.elapsed().as_secs_f64(), serde_json::to_string_pretty(&minutes).unwrap());
}
