//! Multi-turn tool-use requests for chat, in two wire formats: Anthropic's Messages API with
//! native `tool_use` / `tool_result` blocks, and OpenAI-compatible function calling (Ollama,
//! LM Studio, OpenAI, OpenRouter, Gemini's /openai endpoint).
//!
//! Interactive only: never goes through the Batch API. Shares `LlmClient`'s HTTP client,
//! endpoints, key and call log. Every request is checked against `MAX_INPUT_TOKENS` before it
//! is sent; the chat loop (features::chat::engine) trims the conversation to fit first.

use super::client::{estimate_tokens, truncate, HttpError, LlmClient, Part, MAX_INPUT_TOKENS};
use crate::config::{ProviderKind, Reasoning};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::future::Future;
use std::time::{Duration, Instant};

/// A tool the model may call. `parameters` is a JSON Schema object.
#[derive(Debug, Clone)]
pub struct ToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    pub parameters: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
    /// Set when the model's arguments were not valid JSON (OpenAI format sends them as a string).
    pub arg_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolResult {
    pub call_id: String,
    pub content: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Msg {
    User(String),
    /// `raw` is the provider's assistant output as received (Anthropic: the content array;
    /// OpenAI: `{"tool_calls": [...]}` normalized). It is echoed back unchanged so thinking
    /// blocks and provider extras (signatures) survive the tool loop. Earlier turns loaded from
    /// the database have no `raw`.
    Assistant { text: String, calls: Vec<ToolCall>, raw: Option<Value> },
    ToolResults(Vec<ToolResult>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    EndTurn,
    ToolUse,
    MaxTokens,
    Refusal,
    Other,
}

#[derive(Debug, Clone)]
pub struct Reply {
    pub text: String,
    pub calls: Vec<ToolCall>,
    pub stop: Stop,
    pub raw: Value,
    /// Input and output tokens as billed (Anthropic: uncached input only).
    pub usage: (Option<i64>, Option<i64>),
    /// Every input token of the request, cached ones included; compared with the estimate.
    pub prompt_tokens: Option<i64>,
}

pub struct ChatRequest<'a> {
    pub system: &'a str,
    pub messages: &'a [Msg],
    pub tools: &'a [ToolSpec],
    pub max_tokens: u32,
}

/// Sends one model turn. Implemented by `ChatApi`; tests use a scripted fake.
pub trait ChatTransport: Sync {
    fn send(&self, req: &ChatRequest<'_>) -> impl Future<Output = Result<Reply>> + Send;
}

/// Provider-neutral input-token estimate for a request. Prose (system prompt, questions,
/// answers, tool descriptions) counts 3.5 characters per token like `estimate_tokens`; JSON
/// (tool results, tool schemas, tool calls and echoed assistant blocks) counts 3, because quotes,
/// braces, keys and numbers split into more tokens. The chat loop also scales this estimate by
/// what the provider reported for the previous round.
pub fn estimate_request_tokens(req: &ChatRequest<'_>) -> usize {
    let mut prose = req.system.chars().count();
    let mut json = 0;
    for t in req.tools {
        prose += t.name.len() + t.description.chars().count();
        json += t.parameters.to_string().chars().count() + 20;
    }
    for m in req.messages {
        json += 20;
        match m {
            Msg::User(s) => prose += s.chars().count(),
            Msg::Assistant { text, calls, raw } => match raw {
                Some(r) => json += r.to_string().chars().count(),
                None => {
                    prose += text.chars().count();
                    json += calls.iter().map(|c| c.name.len() + c.input.to_string().chars().count() + 30).sum::<usize>();
                }
            },
            Msg::ToolResults(rs) => json += rs.iter().map(|r| r.content.chars().count() + 40).sum::<usize>(),
        }
    }
    estimate_tokens(&[Part::Text(String::new())], None) + (prose * 2).div_ceil(7) + json.div_ceil(3)
}

pub struct ChatApi {
    pub client: LlmClient,
    /// Total transport attempts for retryable errors (429, 5xx, network).
    pub attempts: u32,
}

impl ChatApi {
    pub fn new(client: LlmClient) -> ChatApi {
        ChatApi { client, attempts: 2 }
    }

    /// Builds the request for the client's wire format. `thinking_field` lets the caller drop
    /// the `thinking` field after a model rejects it (Opus 5.5 refuses `disabled`).
    pub fn build(&self, req: &ChatRequest<'_>, thinking_field: bool) -> (String, Value) {
        match self.client.kind {
            ProviderKind::Anthropic => (
                self.client.anthropic_messages_url(),
                anthropic_body(&self.client.model, self.client.reasoning, req, thinking_field, self.client.host_name() == "api.anthropic.com"),
            ),
            _ => (self.client.chat_completions_url(), openai_body(&self.client.model, &self.client.host_name(), req)),
        }
    }

    async fn post(&self, url: &str, body: &Value) -> Result<Value> {
        let c = &self.client;
        let mut rb = c.http_client().post(url).json(body);
        match c.kind {
            ProviderKind::Anthropic => {
                let key = c.key().ok_or_else(|| anyhow!("No Anthropic API key configured"))?;
                rb = rb.header("x-api-key", key).header("anthropic-version", "2023-06-01");
            }
            ProviderKind::LmStudio => rb = rb.bearer_auth(c.key().unwrap_or("lm-studio")),
            _ => {
                if let Some(k) = c.key() {
                    rb = rb.bearer_auth(k);
                }
            }
        }
        let resp = rb.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        if !status.is_success() {
            return Err(HttpError { status: status.as_u16(), body: text }.into());
        }
        serde_json::from_str(&text).map_err(|e| anyhow!("Invalid JSON from provider: {e}: {}", truncate(&text, 300)))
    }

    async fn send_impl(&self, req: &ChatRequest<'_>) -> Result<Reply> {
        let estimate = estimate_request_tokens(req);
        if estimate > MAX_INPUT_TOKENS {
            bail!("Request would be about {estimate} input tokens, over the {MAX_INPUT_TOKENS} limit; not sent");
        }
        let mut thinking_field = true;
        let mut last_err = anyhow!("no attempts made");
        let mut attempt = 0;
        while attempt < self.attempts.max(1) {
            if attempt > 0 {
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            let (url, body) = self.build(req, thinking_field);
            let started = Instant::now();
            let result = self.post(&url, &body).await.and_then(|v| parse_reply(self.client.kind, &v));
            if let Some(db) = self.client.call_log() {
                let latency = started.elapsed().as_millis() as i64;
                let req_log = LlmClient::redacted(&body);
                match &result {
                    Ok(r) => db.log_llm_call(None, "chat", &self.client.model, latency, &req_log, Some(&r.raw.to_string()), None, r.usage),
                    Err(e) => db.log_llm_call(None, "chat", &self.client.model, latency, &req_log, None, Some(&e.to_string()), (None, None)),
                }
            }
            match result {
                Ok(r) => return Ok(r),
                Err(e) => {
                    log::warn!("chat request attempt {} failed: {e:#}", attempt + 1);
                    // Some models refuse `thinking: disabled`; resend once without the field.
                    if thinking_field && body.get("thinking").is_some() && rejects_thinking(&e) {
                        thinking_field = false;
                        last_err = e;
                        continue;
                    }
                    let retry = LlmClient::is_retryable(&e);
                    last_err = e;
                    if !retry {
                        break;
                    }
                }
            }
            attempt += 1;
        }
        Err(last_err)
    }
}

impl ChatTransport for ChatApi {
    fn send(&self, req: &ChatRequest<'_>) -> impl Future<Output = Result<Reply>> + Send {
        self.send_impl(req)
    }
}

fn rejects_thinking(e: &anyhow::Error) -> bool {
    e.downcast_ref::<HttpError>().is_some_and(|h| h.status == 400 && h.body.contains("thinking"))
}

// ---- Anthropic Messages API ----

pub fn anthropic_body(model: &str, reasoning: Reasoning, req: &ChatRequest<'_>, thinking_field: bool, cache: bool) -> Value {
    let messages: Vec<Value> = req
        .messages
        .iter()
        .map(|m| match m {
            Msg::User(s) => json!({"role": "user", "content": s}),
            Msg::Assistant { raw: Some(raw), .. } if raw.is_array() => json!({"role": "assistant", "content": raw}),
            Msg::Assistant { text, calls, .. } => {
                if calls.is_empty() {
                    json!({"role": "assistant", "content": text})
                } else {
                    let mut blocks = Vec::new();
                    if !text.trim().is_empty() {
                        blocks.push(json!({"type": "text", "text": text}));
                    }
                    for c in calls {
                        blocks.push(json!({"type": "tool_use", "id": c.id, "name": c.name, "input": c.input}));
                    }
                    json!({"role": "assistant", "content": blocks})
                }
            }
            Msg::ToolResults(rs) => json!({
                "role": "user",
                "content": rs.iter().map(|r| {
                    let mut b = json!({"type": "tool_result", "tool_use_id": r.call_id, "content": r.content});
                    if r.is_error {
                        b["is_error"] = json!(true);
                    }
                    b
                }).collect::<Vec<_>>(),
            }),
        })
        .collect();
    let mut body = json!({
        "model": model,
        "max_tokens": req.max_tokens,
        "system": req.system,
        "messages": messages,
    });
    if !req.tools.is_empty() {
        body["tools"] = json!(req
            .tools
            .iter()
            .map(|t| json!({"name": t.name, "description": t.description, "input_schema": t.parameters}))
            .collect::<Vec<_>>());
    }
    // Same reasoning mapping as LlmClient; older Claude generations do not know these fields.
    if !model.starts_with("claude-3") {
        match reasoning {
            Reasoning::Off => {
                if thinking_field {
                    body["thinking"] = json!({"type": "disabled"});
                }
                body["output_config"] = json!({"effort": "low"});
            }
            Reasoning::Low => body["output_config"] = json!({"effort": "low"}),
            Reasoning::Default => {}
        }
    }
    // Automatic prompt caching: each tool round re-sends the same prefix.
    if cache {
        body["cache_control"] = json!({"type": "ephemeral"});
    }
    body
}

fn anthropic_reply(v: &Value) -> Result<Reply> {
    let blocks = v["content"].as_array().ok_or_else(|| anyhow!("Response has no content: {}", truncate(&v.to_string(), 300)))?;
    let text = blocks.iter().filter(|b| b["type"] == "text").filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join("");
    let calls = blocks
        .iter()
        .filter(|b| b["type"] == "tool_use")
        .map(|b| ToolCall {
            id: b["id"].as_str().unwrap_or_default().to_string(),
            name: b["name"].as_str().unwrap_or_default().to_string(),
            input: if b["input"].is_object() { b["input"].clone() } else { json!({}) },
            arg_error: None,
        })
        .collect();
    let stop = match v["stop_reason"].as_str() {
        Some("end_turn") | Some("stop_sequence") => Stop::EndTurn,
        Some("tool_use") => Stop::ToolUse,
        Some("max_tokens") => Stop::MaxTokens,
        Some("refusal") => Stop::Refusal,
        _ => Stop::Other,
    };
    let u = &v["usage"];
    let input = u["input_tokens"].as_i64();
    // With prompt caching, input_tokens leaves out the cached part of the prompt.
    let prompt_tokens = input.map(|i| i + u["cache_creation_input_tokens"].as_i64().unwrap_or(0) + u["cache_read_input_tokens"].as_i64().unwrap_or(0));
    Ok(Reply { text, calls, stop, raw: v["content"].clone(), usage: (input, u["output_tokens"].as_i64()), prompt_tokens })
}

// ---- OpenAI-compatible chat completions ----

pub fn openai_body(model: &str, host: &str, req: &ChatRequest<'_>) -> Value {
    let mut messages = vec![json!({"role": "system", "content": req.system})];
    for m in req.messages {
        match m {
            Msg::User(s) => messages.push(json!({"role": "user", "content": s})),
            Msg::Assistant { text, calls, raw } => {
                let mut msg = json!({"role": "assistant", "content": text});
                let raw_calls = raw.as_ref().and_then(|r| r.get("tool_calls")).filter(|c| c.as_array().is_some_and(|a| !a.is_empty()));
                if let Some(rc) = raw_calls {
                    msg["tool_calls"] = rc.clone();
                } else if !calls.is_empty() {
                    msg["tool_calls"] = json!(calls
                        .iter()
                        .map(|c| json!({"id": c.id, "type": "function", "function": {"name": c.name, "arguments": c.input.to_string()}}))
                        .collect::<Vec<_>>());
                }
                messages.push(msg);
            }
            Msg::ToolResults(rs) => {
                for r in rs {
                    messages.push(json!({"role": "tool", "tool_call_id": r.call_id, "content": r.content}));
                }
            }
        }
    }
    let mut body = json!({"model": model, "messages": messages, "stream": false});
    if !req.tools.is_empty() {
        body["tools"] = json!(req
            .tools
            .iter()
            .map(|t| json!({"type": "function", "function": {"name": t.name, "description": t.description, "parameters": t.parameters}}))
            .collect::<Vec<_>>());
    }
    // Same parameter rules as LlmClient.
    if host == "api.openai.com" {
        body["max_completion_tokens"] = json!(req.max_tokens);
    } else {
        body["max_tokens"] = json!(req.max_tokens);
        body["temperature"] = json!(0.7);
    }
    if host == "openrouter.ai" {
        body["reasoning"] = json!({"effort": "low"});
    }
    body
}

fn openai_reply(v: &Value) -> Result<Reply> {
    let choice = &v["choices"][0];
    if choice.is_null() {
        let err = v["error"]["message"].as_str().or(v["error"].as_str()).map(String::from).unwrap_or_else(|| truncate(&v.to_string(), 300));
        return Err(HttpError { status: 502, body: err }.into());
    }
    let msg = &choice["message"];
    let text = match &msg["content"] {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join(""),
        _ => String::new(),
    };
    let mut calls = Vec::new();
    let mut raw_calls = Vec::new();
    for (i, tc) in msg["tool_calls"].as_array().map(|a| a.as_slice()).unwrap_or_default().iter().enumerate() {
        // Some local servers omit ids; the tool message must still reference one.
        let id = tc["id"].as_str().filter(|s| !s.is_empty()).map(String::from).unwrap_or_else(|| format!("call_{i}"));
        let name = tc["function"]["name"].as_str().unwrap_or_default().to_string();
        let (input, arg_error) = match &tc["function"]["arguments"] {
            Value::String(s) if s.trim().is_empty() => (json!({}), None),
            Value::String(s) => match serde_json::from_str::<Value>(s) {
                Ok(v) if v.is_object() => (v, None),
                Ok(_) => (json!({}), Some("arguments must be a JSON object".to_string())),
                Err(e) => (json!({}), Some(format!("arguments are not valid JSON: {e}"))),
            },
            Value::Object(_) => (tc["function"]["arguments"].clone(), None),
            _ => (json!({}), None),
        };
        // Echo the call as received (keeps extras such as thought signatures), with the id
        // filled in and the arguments as a string, which every server accepts on input.
        let mut rc = tc.clone();
        rc["id"] = json!(id);
        rc["type"] = json!("function");
        if !rc["function"]["arguments"].is_string() {
            rc["function"]["arguments"] = json!(input.to_string());
        }
        raw_calls.push(rc);
        calls.push(ToolCall { id, name, input, arg_error });
    }
    let stop = if !calls.is_empty() {
        Stop::ToolUse
    } else {
        match choice["finish_reason"].as_str() {
            Some("stop") | None => Stop::EndTurn,
            Some("tool_calls") | Some("function_call") => Stop::ToolUse,
            Some("length") => Stop::MaxTokens,
            Some("content_filter") => Stop::Refusal,
            _ => Stop::Other,
        }
    };
    let input = v["usage"]["prompt_tokens"].as_i64();
    Ok(Reply { text, calls, stop, raw: json!({"tool_calls": raw_calls}), usage: (input, v["usage"]["completion_tokens"].as_i64()), prompt_tokens: input })
}

pub fn parse_reply(kind: ProviderKind, v: &Value) -> Result<Reply> {
    match kind {
        ProviderKind::Anthropic => anthropic_reply(v),
        _ => openai_reply(v),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tools() -> Vec<ToolSpec> {
        vec![ToolSpec {
            name: "fetchTimeline",
            description: "Fetch cards",
            parameters: json!({"type": "object", "properties": {"date": {"type": "string"}}}),
        }]
    }

    fn convo() -> Vec<Msg> {
        vec![
            Msg::User("What did I do yesterday?".into()),
            Msg::Assistant {
                text: "Looking.".into(),
                calls: vec![ToolCall { id: "t1".into(), name: "fetchTimeline".into(), input: json!({"date": "2026-10-16"}), arg_error: None }],
                raw: None,
            },
            Msg::ToolResults(vec![ToolResult { call_id: "t1".into(), content: "{\"items\":[]}".into(), is_error: false }]),
        ]
    }

    #[test]
    fn anthropic_request_shape() {
        let msgs = convo();
        let t = tools();
        let req = ChatRequest { system: "sys", messages: &msgs, tools: &t, max_tokens: 1000 };
        let b = anthropic_body("claude-haiku-5-5", Reasoning::Off, &req, true, true);
        assert_eq!(b["system"], "sys");
        assert_eq!(b["tools"][0]["name"], "fetchTimeline");
        assert_eq!(b["tools"][0]["input_schema"]["type"], "object");
        assert_eq!(b["messages"][0], json!({"role": "user", "content": "What did I do yesterday?"}));
        assert_eq!(b["messages"][1]["content"][0], json!({"type": "text", "text": "Looking."}));
        assert_eq!(b["messages"][1]["content"][1]["type"], "tool_use");
        assert_eq!(b["messages"][1]["content"][1]["input"]["date"], "2026-10-16");
        assert_eq!(b["messages"][2]["content"][0]["type"], "tool_result");
        assert_eq!(b["messages"][2]["content"][0]["tool_use_id"], "t1");
        assert!(b["messages"][2]["content"][0].get("is_error").is_none());
        assert_eq!(b["thinking"]["type"], "disabled");
        assert_eq!(b["output_config"]["effort"], "low");
        assert_eq!(b["cache_control"]["type"], "ephemeral");

        let low = anthropic_body("claude-haiku-5-5", Reasoning::Low, &req, true, false);
        assert!(low.get("thinking").is_none());
        assert_eq!(low["output_config"]["effort"], "low");
        assert!(low.get("cache_control").is_none());
        let no_thinking = anthropic_body("claude-haiku-5-5", Reasoning::Off, &req, false, false);
        assert!(no_thinking.get("thinking").is_none());
        let old = anthropic_body("claude-3-5-haiku-latest", Reasoning::Off, &req, true, false);
        assert!(old.get("thinking").is_none() && old.get("output_config").is_none());
    }

    #[test]
    fn anthropic_echoes_raw_assistant_content() {
        let raw = json!([
            {"type": "thinking", "thinking": "", "signature": "sig"},
            {"type": "tool_use", "id": "toolu_1", "name": "fetchTimeline", "input": {"date": "2026-10-16"}}
        ]);
        let msgs = vec![Msg::User("q".into()), Msg::Assistant { text: String::new(), calls: vec![], raw: Some(raw.clone()) }];
        let req = ChatRequest { system: "s", messages: &msgs, tools: &[], max_tokens: 10 };
        let b = anthropic_body("claude-haiku-5-5", Reasoning::Default, &req, true, false);
        assert_eq!(b["messages"][1]["content"], raw);
        assert!(b.get("tools").is_none());
    }

    #[test]
    fn openai_request_shape() {
        let msgs = convo();
        let t = tools();
        let req = ChatRequest { system: "sys", messages: &msgs, tools: &t, max_tokens: 1000 };
        let b = openai_body("qwen3:8b", "localhost", &req);
        assert_eq!(b["messages"][0], json!({"role": "system", "content": "sys"}));
        assert_eq!(b["messages"][2]["role"], "assistant");
        assert_eq!(b["messages"][2]["tool_calls"][0]["function"]["name"], "fetchTimeline");
        assert_eq!(b["messages"][2]["tool_calls"][0]["function"]["arguments"], "{\"date\":\"2026-10-16\"}");
        assert_eq!(b["messages"][3], json!({"role": "tool", "tool_call_id": "t1", "content": "{\"items\":[]}"}));
        assert_eq!(b["tools"][0]["type"], "function");
        assert_eq!(b["tools"][0]["function"]["parameters"]["type"], "object");
        assert_eq!(b["max_tokens"], 1000);
        assert_eq!(b["stream"], false);
        let oa = openai_body("gpt-5-mini", "api.openai.com", &req);
        assert_eq!(oa["max_completion_tokens"], 1000);
        assert!(oa.get("temperature").is_none());
    }

    #[test]
    fn parses_anthropic_tool_use() {
        let v = json!({
            "content": [
                {"type": "thinking", "thinking": "", "signature": "abc"},
                {"type": "text", "text": "Checking yesterday."},
                {"type": "tool_use", "id": "toolu_9", "name": "fetchTimeline", "input": {"date": "2026-10-16"}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 1200, "output_tokens": 40}
        });
        let r = parse_reply(ProviderKind::Anthropic, &v).unwrap();
        assert_eq!(r.text, "Checking yesterday.");
        assert_eq!(r.stop, Stop::ToolUse);
        assert_eq!(r.calls.len(), 1);
        assert_eq!(r.calls[0].id, "toolu_9");
        assert_eq!(r.calls[0].input["date"], "2026-10-16");
        assert_eq!(r.usage, (Some(1200), Some(40)));
        assert_eq!(r.prompt_tokens, Some(1200));
        assert_eq!(r.raw.as_array().unwrap().len(), 3, "thinking block kept for the echo");
        let cached = json!({"content": [], "stop_reason": "end_turn",
            "usage": {"input_tokens": 50, "cache_creation_input_tokens": 1000, "cache_read_input_tokens": 9000, "output_tokens": 5}});
        let r = parse_reply(ProviderKind::Anthropic, &cached).unwrap();
        assert_eq!((r.usage, r.prompt_tokens), ((Some(50), Some(5)), Some(10_050)), "cached input counts toward the prompt size");

        let done = json!({"content": [{"type": "text", "text": "Done"}], "stop_reason": "end_turn", "usage": {}});
        let r = parse_reply(ProviderKind::Anthropic, &done).unwrap();
        assert_eq!((r.text.as_str(), r.stop, r.calls.len()), ("Done", Stop::EndTurn, 0));
        let cut = json!({"content": [], "stop_reason": "max_tokens"});
        assert_eq!(parse_reply(ProviderKind::Anthropic, &cut).unwrap().stop, Stop::MaxTokens);
    }

    #[test]
    fn parses_openai_tool_calls() {
        let v = json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [
                        {"id": "call_a", "type": "function", "function": {"name": "fetchTimeline", "arguments": "{\"date\":\"2026-10-16\"}"}, "extra_content": {"google": {"thought_signature": "x"}}},
                        {"function": {"name": "daySummary", "arguments": {"date": "2026-10-15"}}},
                        {"id": "call_c", "function": {"name": "fetchObservations", "arguments": "{not json"}}
                    ]
                },
                "finish_reason": "stop"
            }],
            "usage": {"prompt_tokens": 900, "completion_tokens": 30}
        });
        let r = parse_reply(ProviderKind::Ollama, &v).unwrap();
        assert_eq!(r.stop, Stop::ToolUse, "tool calls win over finish_reason stop (Ollama)");
        assert_eq!(r.text, "");
        assert_eq!(r.calls[0].input["date"], "2026-10-16");
        assert_eq!(r.calls[1].id, "call_1", "missing id is synthesized");
        assert_eq!(r.calls[1].input["date"], "2026-10-15");
        assert!(r.calls[2].arg_error.is_some());
        assert_eq!(r.raw["tool_calls"][0]["extra_content"]["google"]["thought_signature"], "x");
        assert_eq!(r.raw["tool_calls"][1]["id"], "call_1");
        assert!(r.raw["tool_calls"][1]["function"]["arguments"].is_string());
        assert_eq!(r.usage, (Some(900), Some(30)));
        assert_eq!(r.prompt_tokens, Some(900));

        // The echoed assistant message carries the raw calls with matching ids.
        let msgs = vec![
            Msg::User("q".into()),
            Msg::Assistant { text: r.text.clone(), calls: r.calls.clone(), raw: Some(r.raw.clone()) },
            Msg::ToolResults(vec![ToolResult { call_id: "call_1".into(), content: "{}".into(), is_error: false }]),
        ];
        let req = ChatRequest { system: "s", messages: &msgs, tools: &[], max_tokens: 10 };
        let b = openai_body("m", "localhost", &req);
        assert_eq!(b["messages"][2]["tool_calls"][1]["id"], "call_1");
        assert_eq!(b["messages"][3]["tool_call_id"], "call_1");

        let text = json!({"choices": [{"message": {"content": [{"type": "text", "text": "Hi"}]}, "finish_reason": "length"}]});
        let r = parse_reply(ProviderKind::OpenaiCompatible, &text).unwrap();
        assert_eq!((r.text.as_str(), r.stop), ("Hi", Stop::MaxTokens));
        let err = json!({"error": {"message": "model not found"}});
        assert!(parse_reply(ProviderKind::OpenaiCompatible, &err).unwrap_err().to_string().contains("model not found"));
    }

    #[test]
    fn estimate_grows_with_content_and_guard_blocks_oversize() {
        let small = vec![Msg::User("hi".into())];
        let req = ChatRequest { system: "s", messages: &small, tools: &[], max_tokens: 10 };
        let a = estimate_request_tokens(&req);
        let big = vec![Msg::User("x".repeat(350_000))];
        let req2 = ChatRequest { system: "s", messages: &big, tools: &[], max_tokens: 10 };
        let b = estimate_request_tokens(&req2);
        assert!(a < 100 && b > MAX_INPUT_TOKENS, "{a} {b}");
        // The same number of characters as a JSON tool result counts at 3 per token, not 3.5.
        let text = vec![Msg::User("x".repeat(42_000))];
        let tool = vec![Msg::ToolResults(vec![ToolResult { call_id: "t".into(), content: "x".repeat(42_000), is_error: false }])];
        let as_text = estimate_request_tokens(&ChatRequest { system: "", messages: &text, tools: &[], max_tokens: 0 });
        let as_json = estimate_request_tokens(&ChatRequest { system: "", messages: &tool, tools: &[], max_tokens: 0 });
        assert!((11_990..12_100).contains(&as_text), "{as_text}");
        assert!((13_990..14_100).contains(&as_json), "{as_json}");

        // The transport refuses an oversize request before any network I/O.
        let client = LlmClient::new(ProviderKind::Ollama, "http://127.0.0.1:9", "m", None, None).unwrap();
        let api = ChatApi::new(client);
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let err = rt.block_on(api.send(&req2)).unwrap_err();
        assert!(err.to_string().contains("not sent"), "{err}");
    }

    #[test]
    fn thinking_rejection_is_detected() {
        let e: anyhow::Error = HttpError { status: 400, body: "thinking.type: disabled is not supported".into() }.into();
        assert!(rejects_thinking(&e));
        let e: anyhow::Error = HttpError { status: 401, body: "thinking".into() }.into();
        assert!(!rejects_thinking(&e));
    }
}
