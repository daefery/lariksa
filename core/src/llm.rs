//! The model that writes the notulen: a local llama-server, or an
//! OpenAI-compatible API (OpenAI, CLIProxyAPI, a LAN server) set in Settings.
//!
//! Local: the server starts only for the notulen step and stops right after, so
//! the 4B model and the Whisper model are never resident together. llama.cpp's
//! ``--fit`` sizes context and GPU offload to the memory that is actually free,
//! and the JSON-schema grammar guarantees parseable output.
//!
//! API: the same prompts and schemas go to ``<base>/chat/completions``; only the
//! transcript text leaves the laptop, never the audio.

use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use crate::catalog::http_agent;
use crate::hardware::Device;
use crate::i18n::tr;
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

fn messages(system: &str, user: &str) -> Value {
    json!([{"role": "system", "content": system}, {"role": "user", "content": user}])
}

/// The model's answer as JSON. Tolerates a reply wrapped in a ```json fence,
/// which some APIs add when they ignore ``response_format``.
fn parse_answer(content: &str) -> Option<Value> {
    serde_json::from_str(content).ok().or_else(|| {
        let (start, end) = (content.find('{')?, content.rfind('}')?);
        serde_json::from_str(content.get(start..=end)?).ok()
    })
}

/// The start of an error reply, with the key blanked out: errors are saved with
/// the meeting and in the log, and some APIs quote the key they rejected.
fn error_text(response: ureq::Response, key: &str) -> String {
    let text: String = response.into_string().unwrap_or_default().chars().take(300).collect();
    if key.len() >= 4 { text.replace(key, "***") } else { text }
}

/// A failed call in words for the screen and the log.
fn describe(err: ureq::Error, key: &str) -> String {
    match err {
        ureq::Error::Status(code, r) => format!("{} {code}: {}", tr("Mesin AI error", "AI engine error"), error_text(r, key)),
        e => format!("{}: {e}", tr("Mesin AI tidak merespons", "The AI engine is not responding")),
    }
}

/// A failure worth one more try: the request stalled or dropped, or the server
/// was busy (429) or broken (5xx). Measured through CLIProxyAPI: about one call in
/// four stalled for minutes, and the same request sent again answered in under a minute.
fn transient(err: &ureq::Error) -> bool {
    match err {
        ureq::Error::Status(code, _) => *code == 429 || *code >= 500,
        ureq::Error::Transport(_) => true,
    }
}

/// One schema-constrained answer. Retries once when the output is not valid JSON
/// or the call failed in a way another try can fix.
///
/// Locally the grammar guarantees valid JSON only if generation finishes; when it
/// stops at max_tokens the text ends mid-string. The retry payload uses a higher
/// temperature and more tokens, which in practice yields a shorter, complete answer.
fn complete_with_retry(agent: &ureq::Agent, url: &str, key: &str, payload: impl Fn(usize) -> Value) -> Result<Value> {
    for attempt in 0..2 {
        let mut request = agent.post(url);
        if !key.is_empty() {
            request = request.set("Authorization", &format!("Bearer {key}"));
        }
        let body: Value = match request.send_json(payload(attempt)) {
            Ok(r) => r.into_json()?,
            Err(e) if attempt == 0 && transient(&e) => {
                log::warn!("AI call failed, trying once more: {}", describe(e, key));
                continue;
            }
            Err(e) => bail!("{}", describe(e, key)),
        };
        let choice = &body["choices"][0];
        match parse_answer(choice["message"]["content"].as_str().unwrap_or("")) {
            Some(v) => return Ok(v),
            None => log::warn!("Model JSON invalid (finish_reason={}, attempt {})", choice["finish_reason"], attempt + 1),
        }
    }
    Err(anyhow!("{}", tr("Model mengembalikan JSON yang tidak valid.", "The model returned invalid JSON.")))
}

// -- local llama-server ------------------------------------------------------

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
            bail!("{}", tr("Mesin AI lokal belum terpasang.", "The local AI engine is not installed."));
        }
        if !model.exists() {
            bail!("{}", tr("Model notulen belum diunduh.", "The minutes model is not downloaded."));
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
                bail!("{} {}:\n{}", tr("Mesin AI berhenti saat mulai. Lihat", "The AI engine stopped while starting. See"), self.log.display(), self.tail(15));
            }
            if agent.get(&url).call().is_ok_and(|r| r.status() == 200) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        bail!("{}", tr("Mesin AI tidak siap dalam waktu yang wajar.", "The AI engine did not start in time."))
    }

    fn tail(&self, lines: usize) -> String {
        let text = std::fs::read_to_string(&self.log).unwrap_or_default();
        let all: Vec<&str> = text.lines().collect();
        all[all.len().saturating_sub(lines)..].join("\n")
    }

    pub fn complete_json(&self, system: &str, user: &str, schema: Schema) -> Result<Value> {
        let agent = ureq::AgentBuilder::new().timeout_connect(Duration::from_secs(10)).timeout_read(Duration::from_secs(900)).build();
        let url = format!("http://127.0.0.1:{}/v1/chat/completions", self.port);
        complete_with_retry(&agent, &url, &self.key, |attempt| {
            json!({
                "messages": messages(system, user),
                "temperature": if attempt == 0 { 0.2 } else { 0.5 },
                "top_p": 0.9,
                // Small models occasionally loop ("..., L41, L41, L41") until max_tokens and the JSON
                // never closes; penalising recent repeats breaks the loop without hurting the text.
                "repeat_penalty": 1.1,
                "repeat_last_n": 256,
                "max_tokens": if attempt == 0 { 2560 } else { 3328 },
                "response_format": {"type": "json_schema", "json_schema": {"name": "notulen", "schema": schema.json()}},
            })
        })
    }
}

impl Drop for LocalLlm {
    fn drop(&mut self) {
        process::stop(&mut self.child);
    }
}

// -- OpenAI-compatible API ---------------------------------------------------

/// "http://host:8317/v1/", ".../v1/chat/completions" and ".../v1" all name the same API.
pub fn api_base(url: &str) -> String {
    let url = url.trim().trim_end_matches('/');
    url.strip_suffix("/chat/completions").unwrap_or(url).to_string()
}

/// True for an address the app can call: http(s) with a host.
pub fn valid_api_url(url: &str) -> bool {
    let base = api_base(url);
    ["http://", "https://"].iter().any(|p| base.strip_prefix(p).is_some_and(|rest| !rest.is_empty()))
}

/// OpenAI's strict structured output wants ``additionalProperties: false`` on
/// every object; the local grammar does not need it, so the shared schemas omit it.
pub fn strict_schema(schema: &Value) -> Value {
    match schema {
        Value::Object(map) => {
            let mut out: serde_json::Map<String, Value> = map.iter().map(|(k, v)| (k.clone(), strict_schema(v))).collect();
            if out.get("type") == Some(&json!("object")) {
                out.insert("additionalProperties".into(), json!(false));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(strict_schema).collect()),
        other => other.clone(),
    }
}

/// Most action items one API answer may hold. The shared schema allows 10 per
/// transcript part so a local answer closes within its token limit; an API answer
/// has no such limit. On a 12-minute standup 10 filled up before minute 5, 40 filled
/// up before the last speaker, and with room for 80 the model listed 32-43.
pub const API_MAX_ACTIONS: u64 = 80;

/// The schema an API call uses: strict, and without the local model's action cap.
pub fn api_schema(schema: Schema) -> Value {
    let mut s = strict_schema(schema.json());
    if schema == Schema::Actions {
        s["properties"]["actions"]["maxItems"] = json!(API_MAX_ACTIONS);
    }
    s
}

/// Model ids the API offers, sorted, for the Settings picker; also proves the key works.
pub fn list_models(url: &str, key: &str) -> Result<Vec<String>> {
    let mut request = http_agent(Duration::from_secs(20)).get(&format!("{}/models", api_base(url)));
    if !key.is_empty() {
        request = request.set("Authorization", &format!("Bearer {key}"));
    }
    let body: Value = match request.call() {
        Ok(r) => r.into_json()?,
        Err(ureq::Error::Status(code, r)) => {
            let text = error_text(r, key);
            bail!("{} {code}: {text}", tr("API menolak permintaan", "The API refused the request"))
        }
        Err(e) => bail!("{}: {e}", tr("API tidak bisa dihubungi", "The API could not be reached")),
    };
    let mut ids: Vec<String> =
        body["data"].as_array().into_iter().flatten().filter_map(|m| m["id"].as_str().map(String::from)).collect();
    ids.sort();
    Ok(ids)
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApiLlm {
    pub url: String,
    pub key: String,
    pub model: String,
}

impl ApiLlm {
    pub fn complete_json(&self, system: &str, user: &str, schema: Schema) -> Result<Value> {
        let url = format!("{}/chat/completions", api_base(&self.url));
        let schema = api_schema(schema);
        // No sampling or token limits: provider defaults are sound, and some
        // models reject temperature or max_tokens outright.
        // A healthy answer took under 60 s in testing (up to 43 actions from a 12-minute meeting);
        // a stalled one is cut at 150 s and sent once more.
        complete_with_retry(&http_agent(Duration::from_secs(150)), &url, &self.key, |_| {
            json!({
                "model": self.model,
                "messages": messages(system, user),
                "response_format": {"type": "json_schema", "json_schema": {"name": "notulen", "strict": true, "schema": schema}},
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// A one-thread fake API: answers each request in order and reports what it received.
    fn fake_api(replies: Vec<(u16, String)>) -> (String, mpsc::Receiver<(String, String, Value)>) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let base = format!("http://{}/v1/", server.server_addr().to_ip().unwrap());
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for (status, body) in replies {
                let mut req = server.recv().unwrap();
                let auth = req.headers().iter().find(|h| h.field.equiv("Authorization")).map(|h| h.value.to_string()).unwrap_or_default();
                let mut text = String::new();
                req.as_reader().read_to_string(&mut text).unwrap();
                tx.send((req.url().to_string(), auth, serde_json::from_str(&text).unwrap_or(Value::Null))).unwrap();
                req.respond(tiny_http::Response::from_string(body).with_status_code(status)).unwrap();
            }
        });
        (base, rx)
    }

    fn reply(content: &str) -> (u16, String) {
        (200, json!({"choices": [{"message": {"content": content}, "finish_reason": "stop"}]}).to_string())
    }

    #[test]
    fn base_url_accepts_the_usual_spellings() {
        for url in ["http://127.0.0.1:8317/v1/", "http://127.0.0.1:8317/v1", " http://127.0.0.1:8317/v1/chat/completions "] {
            assert_eq!(api_base(url), "http://127.0.0.1:8317/v1");
        }
        assert!(valid_api_url("https://api.openai.com/v1"));
        assert!(!valid_api_url("api.openai.com/v1"));
        assert!(!valid_api_url("http://"));
        assert!(!valid_api_url(""));
    }

    #[test]
    fn strict_schema_closes_every_object_and_keeps_the_rest() {
        let s = strict_schema(Schema::Topics.json());
        assert_eq!(s["additionalProperties"], false);
        assert_eq!(s["properties"]["chapters"]["items"]["additionalProperties"], false);
        assert_eq!(s["properties"]["chapters"]["maxItems"], 5);
        assert!(Schema::Topics.json().get("additionalProperties").is_none(), "the shared schema is untouched");
    }

    #[test]
    fn api_call_sends_model_key_and_strict_schema() {
        let (base, seen) = fake_api(vec![reply(r#"{"title":"Anggaran"}"#)]);
        let llm = ApiLlm { url: base, key: "sk-test".into(), model: "gpt-x".into() };
        assert_eq!(llm.complete_json("sys", "user", Schema::Overview).unwrap(), json!({"title": "Anggaran"}));
        let (path, auth, body) = seen.recv().unwrap();
        assert_eq!(path, "/v1/chat/completions");
        assert_eq!(auth, "Bearer sk-test");
        assert_eq!(body["model"], "gpt-x");
        assert_eq!(body["messages"][1]["content"], "user");
        assert_eq!(body["response_format"]["json_schema"]["schema"]["additionalProperties"], false);
        assert!(body.get("temperature").is_none() && body.get("max_tokens").is_none());
    }

    #[test]
    fn api_answers_may_hold_more_actions_than_local_ones() {
        assert_eq!(Schema::Actions.json()["properties"]["actions"]["maxItems"], 10, "local cap unchanged");
        assert_eq!(api_schema(Schema::Actions)["properties"]["actions"]["maxItems"], API_MAX_ACTIONS);
        assert_eq!(api_schema(Schema::Topics), strict_schema(Schema::Topics.json()), "other schemas untouched");
        let (base, seen) = fake_api(vec![reply(r#"{"actions":[]}"#)]);
        ApiLlm { url: base, key: String::new(), model: "m".into() }.complete_json("s", "u", Schema::Actions).unwrap();
        assert_eq!(seen.recv().unwrap().2["response_format"]["json_schema"]["schema"]["properties"]["actions"]["maxItems"], API_MAX_ACTIONS);
    }

    #[test]
    fn api_without_key_sends_no_authorization() {
        let (base, seen) = fake_api(vec![reply("{}")]);
        ApiLlm { url: base, key: String::new(), model: "m".into() }.complete_json("s", "u", Schema::Overview).unwrap();
        assert_eq!(seen.recv().unwrap().1, "");
    }

    #[test]
    fn fenced_or_broken_answers_are_recovered_or_retried() {
        let (base, _seen) = fake_api(vec![reply("not json"), reply("```json\n{\"ok\": 1}\n```")]);
        let llm = ApiLlm { url: base, key: "k".into(), model: "m".into() };
        assert_eq!(llm.complete_json("s", "u", Schema::Overview).unwrap(), json!({"ok": 1}));
    }

    #[test]
    fn a_busy_or_broken_server_gets_one_more_try() {
        let (base, seen) = fake_api(vec![(503, "overloaded".into()), reply(r#"{"ok":2}"#)]);
        let llm = ApiLlm { url: base, key: "k".into(), model: "m".into() };
        assert_eq!(llm.complete_json("s", "u", Schema::Overview).unwrap(), json!({"ok": 2}));
        assert_eq!(seen.iter().take(2).count(), 2);
        let (base, _seen) = fake_api(vec![(429, "slow down".into()), (502, "bad gateway".into())]);
        let err = ApiLlm { url: base, key: "k".into(), model: "m".into() }.complete_json("s", "u", Schema::Overview).unwrap_err();
        assert!(err.to_string().contains("502"), "{err}");
    }

    #[test]
    fn api_errors_show_the_status_and_message() {
        // A rejected key is not retried: one request only.
        let (base, _seen) = fake_api(vec![(401, r#"{"error":"Missing API key"}"#.into())]);
        let err = ApiLlm { url: base, key: String::new(), model: "m".into() }.complete_json("s", "u", Schema::Overview).unwrap_err();
        assert!(err.to_string().contains("401") && err.to_string().contains("Missing API key"), "{err}");
    }

    #[test]
    fn a_key_quoted_in_an_error_is_blanked() {
        let (base, _seen) = fake_api(vec![(401, r#"{"error":"Incorrect API key provided: sk-secret-123"}"#.into())]);
        let err = ApiLlm { url: base, key: "sk-secret-123".into(), model: "m".into() }.complete_json("s", "u", Schema::Overview).unwrap_err();
        assert!(!err.to_string().contains("sk-secret-123") && err.to_string().contains("***"), "{err}");
    }

    #[test]
    fn models_are_listed_sorted() {
        let (base, seen) = fake_api(vec![(200, json!({"data": [{"id": "gpt-b"}, {"id": "claude-a"}]}).to_string())]);
        assert_eq!(list_models(&base, "k").unwrap(), ["claude-a", "gpt-b"]);
        assert_eq!(seen.recv().unwrap().0, "/v1/models");
    }
}
