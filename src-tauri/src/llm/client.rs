//! One client for two wire formats: Anthropic's Messages API and OpenAI-compatible chat
//! completions (Ollama, LM Studio, OpenAI, OpenRouter, Gemini's /openai endpoint).

use crate::config::ProviderKind;
use crate::db::Db;
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum Part {
    Text(String),
    /// Base64-encoded JPEG.
    Jpeg(String),
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
        })
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
                        Part::Jpeg(b) => json!({"type": "image", "source": {"type": "base64", "media_type": "image/jpeg", "data": b}}),
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
                (self.messages_url(), body)
            }
            _ => {
                let content: Vec<Value> = parts
                    .iter()
                    .map(|p| match p {
                        Part::Text(t) => json!({"type": "text", "text": t}),
                        Part::Jpeg(b) => json!({"type": "image_url", "image_url": {"url": format!("data:image/jpeg;base64,{b}")}}),
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

    async fn send_once(&self, url: &str, body: &Value) -> Result<(String, (Option<i64>, Option<i64>))> {
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
            ProviderKind::Anthropic => {
                let out: String = v["content"]
                    .as_array()
                    .map(|blocks| blocks.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join(""))
                    .unwrap_or_default();
                if v["stop_reason"] == "max_tokens" {
                    bail!("Response was cut off at max_tokens");
                }
                let usage = (v["usage"]["input_tokens"].as_i64(), v["usage"]["output_tokens"].as_i64());
                Ok((out, usage))
            }
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
        let (url, body) = self.build_request(&parts, &opts);
        let mut last_err = anyhow!("no attempts made");
        for attempt in 0..opts.attempts.max(1) {
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
                    log::warn!("{} attempt {} failed: {e:#}", opts.op, attempt + 1);
                    let retry = Self::retryable(&e);
                    last_err = e;
                    if !retry {
                        break;
                    }
                }
            }
        }
        Err(last_err)
    }

    /// Lists model ids from the provider (`/models`), for the settings dropdown.
    pub async fn list_models(&self) -> Result<Vec<String>> {
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
