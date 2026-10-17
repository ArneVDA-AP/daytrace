//! One client for two wire formats: Anthropic's Messages API and OpenAI-compatible chat
//! completions (Ollama, LM Studio, OpenAI, OpenRouter, Gemini's /openai endpoint).

use crate::config::ProviderKind;
use crate::db::Db;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum Part {
    Text(String),
    /// Base64-encoded JPEG with its pixel size (for token estimates).
    Jpeg { b64: String, w: u32, h: u32 },
}

/// Hard ceiling per request. Haiku bills 5× once a prompt passes 100k input tokens; 90k leaves
/// room for estimate error.
pub const MAX_INPUT_TOKENS: usize = 90_000;

/// Rough input-token estimate: images by 28 px patches, text at ~3.5 characters per token.
pub fn estimate_tokens(parts: &[Part], system: Option<&str>) -> usize {
    let text_chars: usize = parts
        .iter()
        .map(|p| if let Part::Text(t) = p { t.chars().count() } else { 0 })
        .sum::<usize>()
        + system.map(|s| s.chars().count()).unwrap_or(0);
    let images: usize = parts
        .iter()
        .map(|p| if let Part::Jpeg { w, h, .. } = p { crate::imaging::image_tokens(*w, *h) } else { 0 })
        .sum();
    images + text_chars * 2 / 7 + 50
}

#[derive(Debug, Clone)]
pub struct CallOpts {
    pub op: &'static str,
    pub batch_id: Option<i64>,
    pub max_tokens: u32,
    pub system: Option<String>,
    /// Total transport attempts (1 = no retry). Backoff 2 s, 4 s, 8 s.
    pub attempts: u32,
}

#[derive(Clone)]
pub struct LlmClient {
    http: reqwest::Client,
    pub kind: ProviderKind,
    pub base_url: String,
    pub model: String,
    api_key: Option<String>,
    db: Option<Db>,
    /// Route Anthropic requests through the Message Batches API.
    pub use_batch: bool,
    pub reasoning: crate::config::Reasoning,
    /// The CLI runner for the Claude Code / Codex providers (None for HTTP providers).
    cli: Option<Arc<super::cli::CliBackend>>,
    /// Set after the model rejected `thinking` / `output_config` with HTTP 400 (Claude Opus 5.5
    /// refuses `thinking: {"type": "disabled"}`); later requests leave the field out.
    drop_thinking: Arc<AtomicBool>,
    drop_output_config: Arc<AtomicBool>,
}

#[derive(Debug)]
pub struct HttpError {
    pub status: u16,
    pub body: String,
}
impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "HTTP {}: {}", self.status, truncate(&self.body, 400))
    }
}
impl std::error::Error for HttpError {}

pub fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n { s.to_string() } else { format!("{}…", s.chars().take(n).collect::<String>()) }
}

impl LlmClient {
    pub fn new(kind: ProviderKind, base_url: &str, model: &str, api_key: Option<String>, db: Option<Db>) -> Result<LlmClient> {
        let timeout = match kind {
            ProviderKind::Ollama | ProviderKind::LmStudio => 180,
            _ => 240,
        };
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(timeout))
            .connect_timeout(Duration::from_secs(15))
            .build()?;
        Ok(LlmClient {
            http,
            kind,
            base_url: base_url.trim().trim_end_matches('/').to_string(),
            model: model.trim().to_string(),
            api_key: api_key.filter(|k| !k.trim().is_empty()),
            db,
            use_batch: false,
            reasoning: crate::config::Reasoning::Off,
            cli: super::cli::CliTool::from_kind(kind).map(|tool| Arc::new(super::cli::CliBackend::new(tool, base_url, model))),
            drop_thinking: Arc::new(AtomicBool::new(false)),
            drop_output_config: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Replaces the CLI runner (tests use a fake process runner).
    #[cfg(test)]
    pub(crate) fn with_cli_backend(mut self, backend: super::cli::CliBackend) -> LlmClient {
        self.cli = Some(Arc::new(backend));
        self
    }

    /// The CLI runner, for the Claude Code and Codex providers.
    pub fn cli(&self) -> Option<&super::cli::CliBackend> {
        self.cli.as_deref()
    }

    pub fn is_cli(&self) -> bool {
        self.cli.is_some()
    }

    pub fn is_local(&self) -> bool {
        matches!(self.kind, ProviderKind::Ollama | ProviderKind::LmStudio)
    }

    fn chat_url(&self) -> String {
        let base = &self.base_url;
        if base.ends_with("/chat/completions") {
            return base.clone();
        }
        let has_path = reqwest::Url::parse(base).map(|u| u.path().trim_matches('/').len() > 0).unwrap_or(false);
        if has_path { format!("{base}/chat/completions") } else { format!("{base}/v1/chat/completions") }
    }

    fn messages_url(&self) -> String {
        let base = &self.base_url;
        if base.ends_with("/messages") {
            base.clone()
        } else if base.ends_with("/v1") {
            format!("{base}/messages")
        } else {
            format!("{base}/v1/messages")
        }
    }

    fn models_url(&self) -> String {
        match self.kind {
            ProviderKind::Anthropic => self.messages_url().trim_end_matches("/messages").to_string() + "/models",
            _ => self.chat_url().trim_end_matches("/chat/completions").to_string() + "/models",
        }
    }

    fn host(&self) -> String {
        reqwest::Url::parse(&self.base_url).ok().and_then(|u| u.host_str().map(|h| h.to_lowercase())).unwrap_or_default()
    }

    fn build_request(&self, parts: &[Part], opts: &CallOpts) -> (String, Value) {
        match self.kind {
            ProviderKind::Anthropic => {
                let content: Vec<Value> = parts
                    .iter()
                    .map(|p| match p {
                        Part::Text(t) => json!({"type": "text", "text": t}),
                        Part::Jpeg { b64, .. } => json!({"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": b64}}),
                    })
                    .collect();
                let mut body = json!({
                    "model": self.model,
                    "max_tokens": opts.max_tokens,
                    "messages": [{"role": "user", "content": content}],
                });
                if let Some(s) = &opts.system {
                    body["system"] = json!(s);
                }
                // Older Claude generations do not know these fields. No temperature is sent:
                // Dayflow has no Anthropic API path (its Claude provider is the CLI, which sets
                // none), and the API default suits the JSON tasks.
                let modern = !self.model.starts_with("claude-3");
                if modern {
                    match self.reasoning {
                        crate::config::Reasoning::Off => {
                            body["thinking"] = json!({"type": "disabled"});
                            body["output_config"] = json!({"effort": "low"});
                        }
                        crate::config::Reasoning::Low => {
                            body["output_config"] = json!({"effort": "low"});
                        }
                        crate::config::Reasoning::Default => {}
                    }
                }
                self.apply_reasoning_drops(&mut body);
                (self.messages_url(), body)
            }
            _ => {
                let content: Vec<Value> = parts
                    .iter()
                    .map(|p| match p {
                        Part::Text(t) => json!({"type": "text", "text": t}),
                        Part::Jpeg { b64, .. } => json!({"type": "image_url", "image_url": {"url": format!("data:image/jpeg;base64,{b64}")}}),
                    })
                    .collect();
                let mut messages = Vec::new();
                if let Some(s) = &opts.system {
                    messages.push(json!({"role": "system", "content": s}));
                }
                messages.push(json!({"role": "user", "content": content}));
                let mut body = json!({"model": self.model, "messages": messages, "stream": false});
                let host = self.host();
                if host == "api.openai.com" {
                    // Current OpenAI models reject max_tokens and non-default temperatures.
                    body["max_completion_tokens"] = json!(opts.max_tokens);
                } else {
                    body["max_tokens"] = json!(opts.max_tokens);
                    body["temperature"] = json!(0.7);
                }
                if host == "openrouter.ai" {
                    body["reasoning"] = json!({"effort": "low"});
                }
                (self.chat_url(), body)
            }
        }
    }

    fn redact(body: &Value) -> String {
        // Replace base64 payloads so logs stay small.
        let mut v = body.clone();
        fn walk(v: &mut Value) {
            match v {
                Value::Object(m) => {
                    for (k, val) in m.iter_mut() {
                        if (k == "data" || k == "url") && val.as_str().map(|s| s.len() > 512).unwrap_or(false) {
                            *val = json!(format!("<{} bytes base64>", val.as_str().unwrap().len()));
                        } else {
                            walk(val);
                        }
                    }
                }
                Value::Array(a) => a.iter_mut().for_each(walk),
                _ => {}
            }
        }
        walk(&mut v);
        v.to_string()
    }

    fn anthropic_headers(&self, req: reqwest::RequestBuilder) -> Result<reqwest::RequestBuilder> {
        let key = self.api_key.clone().ok_or_else(|| anyhow!("No Anthropic API key configured"))?;
        Ok(req.header("x-api-key", key).header("anthropic-version", "2023-06-01"))
    }

    /// One request through the Message Batches API: submit, poll until it ends, read the result.
    async fn send_via_batch(&self, body: &Value) -> Result<(String, (Option<i64>, Option<i64>))> {
        let base = self.messages_url();
        let batches = format!("{base}/batches");
        let submit = json!({"requests": [{"custom_id": "daytrace", "params": body}]});
        let resp = self.anthropic_headers(self.http.post(&batches).json(&submit))?.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(HttpError { status: status.as_u16(), body: text }.into());
        }
        let v: Value = serde_json::from_str(&text)?;
        let id = v["id"].as_str().ok_or_else(|| anyhow!("Batch API returned no id"))?.to_string();
        log::info!("submitted batch request {id}");

        let started = Instant::now();
        let mut delay = 15u64;
        let results_url = loop {
            tokio::time::sleep(Duration::from_secs(delay)).await;
            delay = (delay + 15).min(60);
            if started.elapsed() > Duration::from_secs(6 * 3600) {
                let _ = self.anthropic_headers(self.http.post(format!("{batches}/{id}/cancel")))?.send().await;
                bail!("Batch request {id} did not finish within 6 hours");
            }
            let r = self.anthropic_headers(self.http.get(format!("{batches}/{id}")))?.send().await;
            let Ok(r) = r else { continue }; // transient network error: keep polling
            if !r.status().is_success() {
                continue;
            }
            let s: Value = r.json().await.unwrap_or_default();
            if s["processing_status"] == "ended" {
                break s["results_url"].as_str().map(String::from).unwrap_or_else(|| format!("{batches}/{id}/results"));
            }
        };
        let r = self.anthropic_headers(self.http.get(&results_url))?.send().await?;
        let status = r.status();
        let jsonl = r.text().await?;
        if !status.is_success() {
            return Err(HttpError { status: status.as_u16(), body: jsonl }.into());
        }
        let line: Value = jsonl
            .lines()
            .filter_map(|l| serde_json::from_str::<Value>(l).ok())
            .find(|l| l["custom_id"] == "daytrace")
            .ok_or_else(|| anyhow!("Batch {id} returned no result"))?;
        let result = &line["result"];
        match result["type"].as_str() {
            Some("succeeded") => Self::parse_anthropic(&result["message"]),
            Some("errored") => {
                let err = &result["error"];
                let kind = err["error"]["type"].as_str().or(err["type"].as_str()).unwrap_or("error");
                // invalid_request_error needs a different request; others may be retried as-is.
                let status = if kind == "invalid_request_error" { 400 } else { 500 };
                Err(HttpError { status, body: err.to_string() }.into())
            }
            other => Err(HttpError { status: 503, body: format!("batch request {}", other.unwrap_or("unknown")) }.into()),
        }
    }

    fn parse_anthropic(v: &Value) -> Result<(String, (Option<i64>, Option<i64>))> {
        let out: String = v["content"]
            .as_array()
            .map(|blocks| blocks.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join(""))
            .unwrap_or_default();
        if v["stop_reason"] == "max_tokens" {
            bail!("Response was cut off at max_tokens");
        }
        Ok((out, (v["usage"]["input_tokens"].as_i64(), v["usage"]["output_tokens"].as_i64())))
    }

    async fn send_once(&self, url: &str, body: &Value) -> Result<(String, (Option<i64>, Option<i64>))> {
        if self.use_batch && self.kind == ProviderKind::Anthropic {
            return self.send_via_batch(body).await;
        }
        let mut req = self.http.post(url).json(body);
        match self.kind {
            ProviderKind::Anthropic => {
                let key = self.api_key.clone().ok_or_else(|| anyhow!("No Anthropic API key configured"))?;
                req = req.header("x-api-key", key).header("anthropic-version", "2023-06-01");
            }
            ProviderKind::LmStudio => {
                req = req.bearer_auth(self.api_key.clone().unwrap_or_else(|| "lm-studio".into()));
            }
            _ => {
                if let Some(k) = &self.api_key {
                    req = req.bearer_auth(k);
                }
            }
        }
        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(HttpError { status: status.as_u16(), body: text }.into());
        }
        let v: Value = serde_json::from_str(&text).map_err(|e| anyhow!("Invalid JSON from provider: {e}: {}", truncate(&text, 300)))?;
        match self.kind {
            ProviderKind::Anthropic => Self::parse_anthropic(&v),
            _ => {
                let msg = &v["choices"][0]["message"]["content"];
                let out = match msg {
                    Value::String(s) => s.clone(),
                    Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join(""),
                    _ => String::new(),
                };
                if v["choices"][0]["finish_reason"] == "length" && out.trim().is_empty() {
                    bail!("Response was cut off at max_tokens");
                }
                let usage = (v["usage"]["prompt_tokens"].as_i64(), v["usage"]["completion_tokens"].as_i64());
                Ok((out, usage))
            }
        }
    }

    fn retryable(err: &anyhow::Error) -> bool {
        if let Some(h) = err.downcast_ref::<HttpError>() {
            return h.status == 429 || h.status == 408 || h.status == 529 || h.status >= 500;
        }
        err.downcast_ref::<reqwest::Error>().is_some()
    }

    pub async fn complete(&self, parts: Vec<Part>, opts: CallOpts) -> Result<String> {
        let estimate = estimate_tokens(&parts, opts.system.as_deref());
        if estimate > MAX_INPUT_TOKENS {
            bail!("Request would be about {estimate} input tokens, over the {MAX_INPUT_TOKENS} limit; not sent");
        }
        if let Some(cli) = self.cli.clone() {
            return self.complete_cli(&cli, parts, opts).await;
        }
        let (url, mut body) = self.build_request(&parts, &opts);
        let mut last_err = anyhow!("no attempts made");
        let mut attempt = 0;
        let mut reasoning_retry_done = false;
        while attempt < opts.attempts.max(1) {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(2u64 << (attempt - 1).min(2))).await;
            }
            let started = Instant::now();
            let result = self.send_once(&url, &body).await;
            let latency = started.elapsed().as_millis() as i64;
            if let Some(db) = &self.db {
                let req_log = Self::redact(&body);
                match &result {
                    Ok((out, usage)) => db.log_llm_call(opts.batch_id, opts.op, &self.model, latency, &req_log, Some(out), None, *usage),
                    Err(e) => db.log_llm_call(opts.batch_id, opts.op, &self.model, latency, &req_log, None, Some(&e.to_string()), (None, None)),
                }
            }
            match result {
                Ok((out, _)) => return Ok(out),
                Err(e) => {
                    // A model that refuses the reasoning fields gets one immediate retry without
                    // them; the client remembers it for later requests.
                    if !reasoning_retry_done && self.kind == ProviderKind::Anthropic && self.note_reasoning_rejection(&e, &body) {
                        reasoning_retry_done = true;
                        log::warn!("{}: model {} rejected the reasoning fields; retrying without them", opts.op, self.model);
                        self.apply_reasoning_drops(&mut body);
                        last_err = e;
                        continue;
                    }
                    log::warn!("{} attempt {} failed: {e:#}", opts.op, attempt + 1);
                    let retry = Self::retryable(&e);
                    last_err = e;
                    attempt += 1;
                    if !retry {
                        break;
                    }
                }
            }
        }
        Err(last_err)
    }

    /// Records which reasoning fields a 400 response complains about. True when the request
    /// carried one of them, so a retry without it can succeed.
    fn note_reasoning_rejection(&self, err: &anyhow::Error, body: &Value) -> bool {
        let Some(h) = err.downcast_ref::<HttpError>() else { return false };
        if h.status != 400 {
            return false;
        }
        let msg = h.body.to_lowercase();
        let has_thinking = body.get("thinking").is_some();
        let has_output_config = body.get("output_config").is_some();
        let mut hit = false;
        if has_thinking && msg.contains("thinking") {
            self.drop_thinking.store(true, Ordering::SeqCst);
            hit = true;
        }
        if has_output_config && (msg.contains("output_config") || msg.contains("effort")) {
            self.drop_output_config.store(true, Ordering::SeqCst);
            hit = true;
        }
        hit
    }

    fn apply_reasoning_drops(&self, body: &mut Value) {
        if let Some(o) = body.as_object_mut() {
            if self.drop_thinking.load(Ordering::SeqCst) {
                o.remove("thinking");
            }
            if self.drop_output_config.load(Ordering::SeqCst) {
                o.remove("output_config");
            }
        }
    }

    /// `complete` for the CLI providers. Retries only failures the classifier calls transient
    /// (an overloaded server, a dropped connection): a signed-out CLI fails the same way every
    /// time, and a run that hit the timeout already spent five minutes of the subscription.
    async fn complete_cli(&self, cli: &super::cli::CliBackend, parts: Vec<Part>, opts: CallOpts) -> Result<String> {
        let timeout = if opts.op == "test" { Duration::from_secs(120) } else { super::cli::run_timeout() };
        let images = parts.iter().filter(|p| matches!(p, Part::Jpeg { .. })).count();
        let text: String = parts.iter().filter_map(|p| if let Part::Text(t) = p { Some(t.as_str()) } else { None }).collect::<Vec<_>>().join("\n");
        let req_log = json!({"cli": cli.tool.bin_name(), "system": opts.system, "prompt": text, "images": images}).to_string();
        let mut last_err = anyhow!("no attempts made");
        for attempt in 0..opts.attempts.max(1) {
            let started = Instant::now();
            let result = cli.complete(&parts, opts.system.as_deref(), timeout).await;
            let latency = started.elapsed().as_millis() as i64;
            if let Some(db) = &self.db {
                match &result {
                    Ok((out, usage)) => {
                        db.log_llm_call(opts.batch_id, opts.op, &self.model, latency, &req_log, Some(out), None, usage.map(|u| u.for_log()).unwrap_or((None, None)))
                    }
                    Err(e) => {
                        let usage = e.downcast_ref::<super::cli::CliFailed>().and_then(|f| f.usage).map(|u| u.for_log()).unwrap_or((None, None));
                        db.log_llm_call(opts.batch_id, opts.op, &self.model, latency, &req_log, None, Some(&format!("{e:#}")), usage)
                    }
                }
            }
            match result {
                Ok((out, _)) => return Ok(out),
                Err(e) => {
                    log::warn!("{} attempt {} via {} failed: {e:#}", opts.op, attempt + 1, cli.tool.display_name());
                    let transient = super::failure::classify(&format!("{e:#}")).kind == super::failure::FailureKind::Transient;
                    let cancelled = format!("{e:#}").contains("cancelled");
                    let timed_out = super::cli::is_timeout(&e);
                    last_err = e;
                    if !transient || cancelled || timed_out {
                        break;
                    }
                }
            }
        }
        Err(last_err)
    }

    /// Lists model ids from the provider (`/models`), for the settings dropdown.
    pub async fn list_models(&self) -> Result<Vec<String>> {
        if let Some(cli) = &self.cli {
            return Ok(cli.tool.suggested_models());
        }
        let mut req = self.http.get(self.models_url());
        match self.kind {
            ProviderKind::Anthropic => {
                let key = self.api_key.clone().ok_or_else(|| anyhow!("No API key"))?;
                req = req.header("x-api-key", key).header("anthropic-version", "2023-06-01");
            }
            _ => {
                if let Some(k) = &self.api_key {
                    req = req.bearer_auth(k);
                }
            }
        }
        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(HttpError { status: status.as_u16(), body: text }.into());
        }
        let v: Value = serde_json::from_str(&text)?;
        let mut ids: Vec<String> = v["data"]
            .as_array()
            .map(|a| a.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect())
            .unwrap_or_default();
        ids.sort();
        Ok(ids)
    }
}

/// Strip code fences, then try the whole text, then the outermost `{…}` or `[…]` slice.
pub fn parse_json<T: serde::de::DeserializeOwned>(text: &str) -> Result<T> {
    let cleaned = text.replace("```json", "").replace("```JSON", "").replace("```", "");
    let cleaned = cleaned.trim();
    let first_err = match serde_json::from_str::<T>(cleaned) {
        Ok(v) => return Ok(v),
        Err(e) => e,
    };
    for (open, close) in [('{', '}'), ('[', ']')] {
        if let (Some(a), Some(b)) = (cleaned.find(open), cleaned.rfind(close)) {
            if b > a {
                if let Ok(v) = serde_json::from_str::<T>(&cleaned[a..=b]) {
                    return Ok(v);
                }
            }
        }
    }
    Err(anyhow!("Could not parse JSON from model output: {first_err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        let c = LlmClient::new(ProviderKind::Ollama, "http://localhost:11434/", "m", None, None).unwrap();
        assert_eq!(c.chat_url(), "http://localhost:11434/v1/chat/completions");
        let c = LlmClient::new(ProviderKind::OpenaiCompatible, "https://generativelanguage.googleapis.com/v1beta/openai", "m", None, None).unwrap();
        assert_eq!(c.chat_url(), "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions");
        let c = LlmClient::new(ProviderKind::Anthropic, "https://api.anthropic.com", "m", None, None).unwrap();
        assert_eq!(c.messages_url(), "https://api.anthropic.com/v1/messages");
        assert_eq!(c.models_url(), "https://api.anthropic.com/v1/models");
    }

    #[test]
    fn lenient_json() {
        #[derive(serde::Deserialize)]
        struct S {
            a: i32,
        }
        assert_eq!(parse_json::<S>("```json\n{\"a\":1}\n```").unwrap().a, 1);
        assert_eq!(parse_json::<S>("Sure! {\"a\":2} done").unwrap().a, 2);
        assert_eq!(parse_json::<Vec<S>>("Here: [{\"a\":3}] ok").unwrap()[0].a, 3);
        assert!(parse_json::<S>("nothing").is_err());
    }
}

#[cfg(test)]
mod estimate_tests {
    use super::*;
    #[test]
    fn estimates_images_and_text() {
        let parts = vec![Part::Text("x".repeat(3500)), Part::Jpeg { b64: String::new(), w: 1720, h: 720 }];
        let e = estimate_tokens(&parts, None);
        assert!(e > 1612 + 900 && e < 1612 + 1200, "{e}");
    }
}

/// Access for `llm::chat_api` (multi-turn tool use), which shares this client's transport,
/// endpoints, key and call log.
impl LlmClient {
    pub(crate) fn http_client(&self) -> &reqwest::Client {
        &self.http
    }
    pub(crate) fn key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }
    pub(crate) fn call_log(&self) -> Option<&Db> {
        self.db.as_ref()
    }
    pub(crate) fn chat_completions_url(&self) -> String {
        self.chat_url()
    }
    pub(crate) fn anthropic_messages_url(&self) -> String {
        self.messages_url()
    }
    pub(crate) fn host_name(&self) -> String {
        self.host()
    }
    pub(crate) fn is_retryable(err: &anyhow::Error) -> bool {
        Self::retryable(err)
    }
    pub(crate) fn redacted(body: &Value) -> String {
        Self::redact(body)
    }
}

/// A local HTTP server with scripted answers, for tests that must go through the real client
/// (no external network).
#[cfg(test)]
pub(crate) mod test_server {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};

    pub struct Server {
        pub url: String,
        pub bodies: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    /// Answers each request with the next (status, body); records the JSON request bodies.
    pub fn start(replies: Vec<(u16, String)>) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let seen = bodies.clone();
        std::thread::spawn(move || {
            for (status, reply) in replies {
                let Ok((mut stream, _)) = listener.accept() else { return };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                let header_end = loop {
                    let n = stream.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        break None;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break Some(i + 4);
                    }
                };
                let Some(header_end) = header_end else { continue };
                let head = String::from_utf8_lossy(&buf[..header_end]).to_lowercase();
                let len: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0)))
                    .unwrap_or(0);
                while buf.len() < header_end + len {
                    let n = stream.read(&mut chunk).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let body = &buf[header_end..(header_end + len).min(buf.len())];
                seen.lock().unwrap().push(serde_json::from_slice(body).unwrap_or(serde_json::Value::Null));
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        Server { url, bodies }
    }
}

#[cfg(test)]
mod reasoning_retry_tests {
    use super::*;
    use crate::config::Reasoning;

    fn ok_reply(text: &str) -> String {
        json!({"content": [{"type": "text", "text": text}], "stop_reason": "end_turn", "usage": {"input_tokens": 3, "output_tokens": 1}}).to_string()
    }

    fn opts() -> CallOpts {
        CallOpts { op: "test", batch_id: None, max_tokens: 50, system: None, attempts: 1 }
    }

    #[tokio::test]
    async fn opus_rejecting_disabled_thinking_is_retried_without_it() {
        let rejection = json!({"type": "error", "error": {"type": "invalid_request_error",
            "message": "thinking.type: 'disabled' is not supported for this model. Use 'adaptive' instead."}})
        .to_string();
        let server = test_server::start(vec![(400, rejection), (200, ok_reply("one")), (200, ok_reply("two"))]);
        let mut c = LlmClient::new(ProviderKind::Anthropic, &server.url, "claude-opus-5-5", Some("k".into()), None).unwrap();
        c.reasoning = Reasoning::Off;
        assert_eq!(c.complete(vec![Part::Text("hi".into())], opts()).await.unwrap(), "one");
        // The client remembers: the next request leaves the field out from the start.
        assert_eq!(c.complete(vec![Part::Text("again".into())], opts()).await.unwrap(), "two");
        let bodies = server.bodies.lock().unwrap().clone();
        assert_eq!(bodies.len(), 3);
        assert_eq!(bodies[0]["thinking"], json!({"type": "disabled"}));
        assert!(bodies[1].get("thinking").is_none());
        // Only the field the error named is dropped; effort stays low.
        assert_eq!(bodies[1]["output_config"], json!({"effort": "low"}));
        assert!(bodies[2].get("thinking").is_none());
        assert!(!bodies.iter().any(|b| b.get("temperature").is_some()), "no temperature to Anthropic");
    }

    #[tokio::test]
    async fn effort_rejection_drops_output_config_and_other_400s_fail() {
        let effort = json!({"type": "error", "error": {"type": "invalid_request_error", "message": "output_config.effort: Extra inputs are not permitted"}}).to_string();
        let other = json!({"type": "error", "error": {"type": "invalid_request_error", "message": "messages: at least one message is required"}}).to_string();
        let server = test_server::start(vec![(400, effort), (200, ok_reply("ok")), (400, other)]);
        let mut c = LlmClient::new(ProviderKind::Anthropic, &server.url, "claude-haiku-5-5", Some("k".into()), None).unwrap();
        c.reasoning = Reasoning::Low;
        assert_eq!(c.complete(vec![Part::Text("hi".into())], opts()).await.unwrap(), "ok");
        let err = c.complete(vec![Part::Text("hi".into())], opts()).await.unwrap_err();
        assert!(err.to_string().contains("HTTP 400"), "{err}");
        let bodies = server.bodies.lock().unwrap().clone();
        assert_eq!(bodies.len(), 3, "an unrelated 400 is not retried");
        assert!(bodies[0].get("output_config").is_some());
        assert!(bodies[1].get("output_config").is_none());
    }

    #[tokio::test]
    async fn cli_provider_goes_through_the_cli() {
        use super::super::cli::{process::FakeRunner, testing, CliTool};
        let tmp = tempfile::tempdir().unwrap();
        let runner = Arc::new(FakeRunner::new(vec![
            FakeRunner::ok(&testing::claude_result("Blue")),
            Err("Claude Code CLI: API Error: 529 Overloaded".into()),
            FakeRunner::ok(&testing::claude_result("Blue again")),
            Err("CLI process timed out after 300 seconds".into()),
            FakeRunner::exit(1, r#"{"type":"result","is_error":true,"result":"Not logged in · Please run /login"}"#, ""),
        ]));
        let db = crate::db::Db::memory();
        let c = LlmClient::new(ProviderKind::ClaudeCli, "", "haiku", None, Some(db.clone()))
            .unwrap()
            .with_cli_backend(testing::backend(CliTool::Claude, runner.clone(), tmp.path()));
        assert_eq!(c.list_models().await.unwrap(), vec!["haiku", "sonnet", "opus"]);
        assert_eq!(c.complete(vec![Part::Text("color?".into())], opts()).await.unwrap(), "Blue");
        // An overloaded server is transient: retried when the caller allows two attempts.
        let two = CallOpts { attempts: 2, ..opts() };
        assert_eq!(c.complete(vec![Part::Text("color?".into())], two.clone()).await.unwrap(), "Blue again");
        // A timeout already spent the whole run: not retried.
        let err = c.complete(vec![Part::Text("color?".into())], two.clone()).await.unwrap_err();
        assert!(super::super::cli::is_timeout(&err), "{err}");
        // Signed out is not retried.
        let err = c.complete(vec![Part::Text("color?".into())], two).await.unwrap_err();
        assert!(err.to_string().contains("Please run /login"));
        assert_eq!(runner.calls().len(), 5);
        assert_eq!(db.token_usage_since(0).unwrap(), (220, 40));
    }
}
