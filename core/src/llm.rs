//! Local LLM through a short-lived llama-server process.
//!
//! The server starts only for the notulen step and stops right after, so the
//! 4B model and the Whisper model are never resident together. llama.cpp's
//! ``--fit`` sizes context and GPU offload to the memory that is actually free,
//! and the JSON-schema grammar guarantees parseable output.

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use crate::hardware::Device;
use crate::process;
use crate::prompts::Schema;
use crate::text::token_urlsafe;

/// A 12k-character transcript chunk is ~3.7k tokens, plus ~0.6k of instructions
/// and up to ~3k of answer. 8k instead of 16k frees enough VRAM on a 4 GB laptop
/// GPU to hold 35/37 layers instead of 29/37 (measured, RTX 3050 Ti).
pub const CONTEXT_TOKENS: u32 = 8192;

fn free_port() -> Result<u16> {
    Ok(TcpListener::bind("127.0.0.1:0")?.local_addr()?.port())
}

fn local_agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout_read(Duration::from_secs(900)).build()
}

pub struct LocalLlm {
    child: Child,
    port: u16,
    /// The server listens on localhost while a meeting transcript is loaded; a
    /// per-run key keeps other local programs and web pages (it allows any CORS
    /// origin) from querying it.
    key: String,
    log: PathBuf,
}

impl LocalLlm {
    pub fn start(server: &Path, model: &Path, device: Option<&Device>) -> Result<LocalLlm> {
        if !server.exists() {
            bail!("Mesin AI lokal belum terpasang.");
        }
        if !model.exists() {
            bail!("Model notulen belum diunduh.");
        }
        let port = free_port()?;
        let key = token_urlsafe(24);
        let log = model.with_file_name("llama-server.log");
        let device_name = device.map_or("none".to_string(), |d| d.name.clone());
        log::info!("Starting llama-server on port {port}, device {device_name}");
        let mut cmd = process::command(server);
        cmd.args(["--model", &model.to_string_lossy()])
            .args(["--host", "127.0.0.1", "--port", &port.to_string(), "--api-key", &key])
            .args(["--ctx-size", &CONTEXT_TOKENS.to_string(), "--parallel", "1", "--no-webui"])
            // Instruct model: no hidden reasoning tokens to wait for.
            .args(["--reasoning-budget", "0", "--device", &device_name])
            // q8 KV cache halves its memory with no visible quality loss, and a
            // smaller --fit margin lets a 4 GB laptop GPU hold more layers.
            .args(["--cache-type-k", "q8_0", "--cache-type-v", "q8_0", "--flash-attn", "on", "--fit-target", "384"])
            .stdin(Stdio::null())
            .stdout(std::fs::File::create(&log)?)
            .stderr(Stdio::from(std::fs::OpenOptions::new().append(true).open(&log)?));
        let child = process::spawn(cmd)?;
        let mut llm = LocalLlm { child, port, key, log };
        llm.wait_ready(Duration::from_secs(180))?;
        Ok(llm)
    }

    fn wait_ready(&mut self, timeout: Duration) -> Result<()> {
        let deadline = Instant::now() + timeout;
        let url = format!("http://127.0.0.1:{}/health", self.port);
        let agent = ureq::AgentBuilder::new().timeout(Duration::from_secs(2)).build();
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                bail!("Mesin AI berhenti saat mulai. Lihat {}:\n{}", self.log.display(), self.tail(15));
            }
            if agent.get(&url).call().is_ok_and(|r| r.status() == 200) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        bail!("Mesin AI tidak siap dalam waktu yang wajar.")
    }

    fn tail(&self, lines: usize) -> String {
        let text = std::fs::read_to_string(&self.log).unwrap_or_default();
        let all: Vec<&str> = text.lines().collect();
        all[all.len().saturating_sub(lines)..].join("\n")
    }

    /// One schema-constrained answer. Retries once when the output is cut off.
    ///
    /// The grammar guarantees valid JSON only if generation finishes; when it
    /// stops at max_tokens the text ends mid-string. The retry uses a higher
    /// temperature and more tokens, which in practice yields a shorter, complete answer.
    pub fn complete_json(&self, system: &str, user: &str, schema: Schema) -> Result<Value> {
        for attempt in 0..2 {
            let payload = json!({
                "messages": [{"role": "system", "content": system}, {"role": "user", "content": user}],
                "temperature": if attempt == 0 { 0.2 } else { 0.5 },
                "top_p": 0.9,
                // Small models occasionally loop ("..., L41, L41, L41") until max_tokens and the JSON
                // never closes; penalising recent repeats breaks the loop without hurting the text.
                "repeat_penalty": 1.1,
                "repeat_last_n": 256,
                "max_tokens": if attempt == 0 { 2560 } else { 3328 },
                "response_format": {"type": "json_schema", "json_schema": {"name": "notulen", "schema": schema.json()}},
            });
            let response = local_agent()
                .post(&format!("http://127.0.0.1:{}/v1/chat/completions", self.port))
                .set("Authorization", &format!("Bearer {}", self.key))
                .send_json(payload);
            let body: Value = match response {
                Ok(r) => r.into_json()?,
                Err(ureq::Error::Status(code, r)) => {
                    let text: String = r.into_string().unwrap_or_default().chars().take(300).collect();
                    bail!("Mesin AI error {code}: {text}")
                }
                Err(e) => bail!("Mesin AI tidak merespons: {e}"),
            };
            let choice = &body["choices"][0];
            let content = choice["message"]["content"].as_str().unwrap_or("");
            match serde_json::from_str::<Value>(content) {
                Ok(v) => return Ok(v),
                Err(_) => log::warn!("Model JSON invalid (finish_reason={}, attempt {})", choice["finish_reason"], attempt + 1),
            }
        }
        Err(anyhow!("Model mengembalikan JSON yang tidak valid."))
    }
}

impl Drop for LocalLlm {
    fn drop(&mut self) {
        process::stop(&mut self.child);
    }
}
