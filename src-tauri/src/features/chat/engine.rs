//! The tool loop (Dayflow's GeminiDirectProvider.runDashboardChatLoop): send the conversation,
//! run every tool the model asks for, send the results back, until the model answers without
//! tools. Each request is trimmed to stay under the 90k input-token ceiling first.

use super::tools::{self, ToolContext, RESULT_MAX_CHARS};
use crate::llm::chat_api::{estimate_request_tokens, ChatRequest, ChatTransport, Msg, Stop, ToolResult, ToolSpec};
use crate::llm::client::MAX_INPUT_TOKENS;
use anyhow::{anyhow, bail, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Dayflow allows 20 tool rounds; each round re-sends the whole conversation, so Daytrace stops
/// at 8 to bound the cost of one question.
pub const MAX_TOOL_ROUNDS: usize = 8;
pub const MAX_OUTPUT_TOKENS: u32 = 8192;
/// Earlier messages of the conversation sent with a new question (oldest dropped first).
pub const MAX_HISTORY_MESSAGES: usize = 40;

pub const ELIDED: &str = "[This earlier tool result was removed to keep the request small. Call the tool again if you still need it.]";
pub const ROUNDS_EXCEEDED: &str = "The assistant exceeded the maximum tool-call rounds. Please try a narrower query.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ToolRun {
    pub name: String,
    pub label: String,
    pub input: Value,
    pub summary: String,
    pub ok: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", tag = "stage")]
pub enum Progress {
    Thinking,
    ToolStarted { name: String, label: String },
    ToolFinished { name: String, summary: String, ok: bool },
}

#[derive(Debug, Clone, Default)]
pub struct Outcome {
    pub text: String,
    pub tool_runs: Vec<ToolRun>,
    pub seen_card_ids: HashSet<i64>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub requests: usize,
}

#[derive(Debug)]
pub struct Cancelled;
impl std::fmt::Display for Cancelled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Stopped.")
    }
}
impl std::error::Error for Cancelled {}

/// Cuts a tool result to the hard cap, saying so to the model.
pub fn cap_result(content: String) -> String {
    if content.len() <= RESULT_MAX_CHARS {
        return content;
    }
    let mut end = RESULT_MAX_CHARS;
    while !content.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n[Result truncated at {RESULT_MAX_CHARS} characters. Use a narrower date range or a limit.]", &content[..end])
}

/// Trims `messages` until the request fits under the ceiling: older tool results first, then
/// the oldest history messages, then the current turn's tool results. `history_len` is the
/// number of leading messages that came from earlier questions; it is updated as they go.
pub fn fit_to_budget(system: &str, tools: &[ToolSpec], messages: &mut Vec<Msg>, history_len: &mut usize, limit: usize) -> Result<()> {
    loop {
        let est = estimate_request_tokens(&ChatRequest { system, messages, tools, max_tokens: 0 });
        if est <= limit {
            return Ok(());
        }
        // 1. Tool results of this turn except the latest batch.
        let last_results = messages.iter().rposition(|m| matches!(m, Msg::ToolResults(_)));
        if elide_one(messages, last_results) {
            continue;
        }
        // 2. Oldest history. Keep the history starting with a user message.
        if *history_len > 0 {
            messages.remove(0);
            *history_len -= 1;
            while *history_len > 0 && !matches!(messages.first(), Some(Msg::User(_))) {
                messages.remove(0);
                *history_len -= 1;
            }
            continue;
        }
        // 3. The latest tool results too.
        if elide_one(messages, None) {
            continue;
        }
        bail!(
            "This question and the data it needs are too large to send (about {est} tokens; the limit is {limit}). Try a shorter question or a narrower date range."
        );
    }
}

fn elide_one(messages: &mut [Msg], skip_index: Option<usize>) -> bool {
    for (i, m) in messages.iter_mut().enumerate() {
        if Some(i) == skip_index {
            continue;
        }
        if let Msg::ToolResults(rs) = m {
            if let Some(r) = rs.iter_mut().find(|r| r.content != ELIDED) {
                r.content = ELIDED.to_string();
                return true;
            }
        }
    }
    false
}

async fn wait_cancel(flag: &AtomicBool) {
    while !flag.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

/// Runs one question to an answer. `history` holds earlier user/assistant text messages.
pub async fn run<T: ChatTransport>(
    transport: &T,
    ctx: &ToolContext,
    system: &str,
    history: Vec<Msg>,
    question: &str,
    cancel: &AtomicBool,
    on_progress: &mut (dyn FnMut(Progress) + Send),
) -> Result<Outcome> {
    let specs = tools::specs();
    let mut history = history;
    if history.len() > MAX_HISTORY_MESSAGES {
        history.drain(..history.len() - MAX_HISTORY_MESSAGES);
    }
    while matches!(history.first(), Some(Msg::Assistant { .. })) {
        history.remove(0);
    }
    let mut history_len = history.len();
    let mut messages = history;
    messages.push(Msg::User(question.to_string()));

    let mut out = Outcome::default();
    let mut interim_text: Vec<String> = Vec::new();
    let mut round = 0;
    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(Cancelled.into());
        }
        fit_to_budget(system, &specs, &mut messages, &mut history_len, MAX_INPUT_TOKENS)?;
        on_progress(Progress::Thinking);
        let req = ChatRequest { system, messages: &messages, tools: &specs, max_tokens: MAX_OUTPUT_TOKENS };
        let reply = tokio::select! {
            r = transport.send(&req) => r?,
            _ = wait_cancel(cancel) => return Err(Cancelled.into()),
        };
        out.requests += 1;
        out.input_tokens += reply.usage.0.unwrap_or(0);
        out.output_tokens += reply.usage.1.unwrap_or(0);

        if reply.calls.is_empty() {
            let mut text = reply.text.trim().to_string();
            if text.is_empty() {
                // The model may have answered before a last tool call.
                text = interim_text.join("\n\n").trim().to_string();
            }
            if text.is_empty() {
                match reply.stop {
                    Stop::MaxTokens => bail!("The answer was cut off before any text was written. Try a narrower question."),
                    Stop::Refusal => bail!("The model declined to answer this question."),
                    _ => bail!("The model returned an empty answer."),
                }
            }
            out.text = text;
            return Ok(out);
        }
        if round >= MAX_TOOL_ROUNDS {
            return Err(anyhow!(ROUNDS_EXCEEDED));
        }
        round += 1;
        if !reply.text.trim().is_empty() {
            interim_text.push(reply.text.trim().to_string());
        }

        // Every tool_use needs a result; identical calls in one reply share one execution
        // (Dayflow's dashboardFunctionCallFingerprint).
        let mut results = Vec::with_capacity(reply.calls.len());
        let mut done: Vec<(String, String, bool)> = Vec::new();
        for call in &reply.calls {
            let fingerprint = format!("{}|{}", call.name, call.input);
            if let Some((_, content, is_error)) = done.iter().find(|(f, _, _)| *f == fingerprint) {
                results.push(ToolResult { call_id: call.id.clone(), content: content.clone(), is_error: *is_error });
                continue;
            }
            let label = tools::label(&call.name, &call.input);
            on_progress(Progress::ToolStarted { name: call.name.clone(), label: label.clone() });
            let output = match &call.arg_error {
                Some(e) => tools::ToolOutput {
                    content: serde_json::json!({"summary": format!("Invalid arguments: {e}"), "error": {"code": "invalid_arguments", "message": e}}).to_string(),
                    summary: format!("Invalid arguments: {e}"),
                    ok: false,
                    card_ids: vec![],
                },
                None => tools::execute(ctx, &call.name, &call.input),
            };
            on_progress(Progress::ToolFinished { name: call.name.clone(), summary: output.summary.clone(), ok: output.ok });
            out.seen_card_ids.extend(output.card_ids.iter().copied());
            out.tool_runs.push(ToolRun { name: call.name.clone(), label, input: call.input.clone(), summary: output.summary.clone(), ok: output.ok });
            let content = cap_result(output.content);
            done.push((fingerprint, content.clone(), !output.ok));
            results.push(ToolResult { call_id: call.id.clone(), content, is_error: !output.ok });
        }
        messages.push(Msg::Assistant { text: reply.text.clone(), calls: reply.calls.clone(), raw: Some(reply.raw.clone()) });
        messages.push(Msg::ToolResults(results));
    }
}

/// `[label](card:123)` links in an answer.
fn card_link_re() -> regex::Regex {
    regex::Regex::new(r"\[([^\]\n]+)\]\(card:/{0,2}(\d+)\)").expect("valid regex")
}

/// Card ids cited in `text`, in order of first appearance.
#[cfg(test)]
pub fn cited_ids(text: &str) -> Vec<i64> {
    let mut seen = Vec::new();
    for c in card_link_re().captures_iter(text) {
        if let Ok(id) = c[2].parse::<i64>() {
            if !seen.contains(&id) {
                seen.push(id);
            }
        }
    }
    seen
}

/// Keeps card links whose id is allowed (seen in tool results or cited earlier) and exists;
/// other links become plain text, so a made-up id never turns into a button.
pub fn sanitize_citations(text: &str, allowed: &HashSet<i64>, exists: impl Fn(i64) -> bool) -> (String, Vec<i64>) {
    let mut kept = Vec::new();
    let cleaned = card_link_re()
        .replace_all(text, |c: &regex::Captures| {
            let id = c[2].parse::<i64>().unwrap_or(-1);
            if allowed.contains(&id) && exists(id) {
                if !kept.contains(&id) {
                    kept.push(id);
                }
                format!("[{}](card:{id})", &c[1])
            } else {
                c[1].to_string()
            }
        })
        .into_owned();
    (cleaned, kept)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::chat::tools::tests::ctx;
    use crate::llm::chat_api::{Reply, ToolCall};
    use serde_json::json;
    use std::sync::Mutex;

    /// Replays scripted replies and records every request it receives.
    struct Script {
        replies: Mutex<Vec<Reply>>,
        seen: Mutex<Vec<Vec<Msg>>>,
    }

    impl Script {
        fn new(replies: Vec<Reply>) -> Script {
            Script { replies: Mutex::new(replies.into_iter().rev().collect()), seen: Mutex::new(vec![]) }
        }
    }

    impl ChatTransport for Script {
        fn send(&self, req: &ChatRequest<'_>) -> impl std::future::Future<Output = Result<Reply>> + Send {
            self.seen.lock().unwrap().push(req.messages.to_vec());
            let next = self.replies.lock().unwrap().pop();
            async move { next.ok_or_else(|| anyhow!("script exhausted")) }
        }
    }

    fn text(t: &str) -> Reply {
        Reply { text: t.into(), calls: vec![], stop: Stop::EndTurn, raw: json!([{"type": "text", "text": t}]), usage: (Some(100), Some(10)) }
    }

    fn call(id: &str, name: &str, input: Value) -> Reply {
        Reply {
            text: String::new(),
            calls: vec![ToolCall { id: id.into(), name: name.into(), input, arg_error: None }],
            stop: Stop::ToolUse,
            raw: json!([]),
            usage: (Some(200), Some(20)),
        }
    }

    fn rt() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap()
    }

    #[test]
    fn runs_tools_then_answers() {
        let c = ctx();
        let script = Script::new(vec![call("t1", "fetchTimeline", json!({"date": "2026-10-16"})), text("You wrote code.")]);
        let mut events = Vec::new();
        let flag = AtomicBool::new(false);
        let out = rt()
            .block_on(run(&script, &c, "sys", vec![Msg::User("old q".into()), Msg::Assistant { text: "old a".into(), calls: vec![], raw: None }], "What did I do?", &flag, &mut |p| events.push(p)))
            .unwrap();
        assert_eq!(out.text, "You wrote code.");
        assert_eq!(out.requests, 2);
        assert_eq!((out.input_tokens, out.output_tokens), (300, 30));
        assert_eq!(out.tool_runs.len(), 1);
        assert!(out.tool_runs[0].ok);
        assert_eq!(out.seen_card_ids.len(), 4);
        let seen = script.seen.lock().unwrap();
        assert_eq!(seen[0].len(), 3, "history plus the question");
        let Msg::ToolResults(rs) = &seen[1][4] else { panic!("tool results last") };
        assert_eq!(rs[0].call_id, "t1");
        assert!(rs[0].content.contains("Writing the chat feature"));
        assert!(matches!(events[1], Progress::ToolStarted { .. }));
    }

    #[test]
    fn stops_after_max_rounds() {
        let c = ctx();
        let replies = (0..=MAX_TOOL_ROUNDS).map(|i| call(&format!("t{i}"), "daySummary", json!({"date": "2026-10-16"}))).collect();
        let script = Script::new(replies);
        let flag = AtomicBool::new(false);
        let err = rt().block_on(run(&script, &c, "sys", vec![], "q", &flag, &mut |_| {})).unwrap_err();
        assert_eq!(err.to_string(), ROUNDS_EXCEEDED);
        assert_eq!(script.seen.lock().unwrap().len(), MAX_TOOL_ROUNDS + 1);
    }

    #[test]
    fn empty_and_cut_off_answers() {
        let c = ctx();
        let flag = AtomicBool::new(false);
        let cut = Reply { text: String::new(), calls: vec![], stop: Stop::MaxTokens, raw: json!([]), usage: (None, None) };
        let err = rt().block_on(run(&Script::new(vec![cut]), &c, "s", vec![], "q", &flag, &mut |_| {})).unwrap_err();
        assert!(err.to_string().contains("cut off"));
        // Text written before a final tool call is used when the last reply is empty.
        let mut first = call("t1", "daySummary", json!({"date": "2026-10-16"}));
        first.text = "Here is your day.".into();
        let empty = Reply { text: String::new(), calls: vec![], stop: Stop::EndTurn, raw: json!([]), usage: (None, None) };
        let out = rt().block_on(run(&Script::new(vec![first, empty]), &c, "s", vec![], "q", &flag, &mut |_| {})).unwrap();
        assert_eq!(out.text, "Here is your day.");
    }

    #[test]
    fn duplicate_calls_share_one_execution_and_errors_are_flagged() {
        let c = ctx();
        let mut r = call("a", "daySummary", json!({"date": "2026-10-16"}));
        r.calls.push(ToolCall { id: "b".into(), name: "daySummary".into(), input: json!({"date": "2026-10-16"}), arg_error: None });
        r.calls.push(ToolCall { id: "c".into(), name: "fetchTimeline".into(), input: json!({}), arg_error: None });
        let script = Script::new(vec![r, text("ok")]);
        let flag = AtomicBool::new(false);
        let out = rt().block_on(run(&script, &c, "s", vec![], "q", &flag, &mut |_| {})).unwrap();
        assert_eq!(out.tool_runs.len(), 2, "duplicate ran once");
        assert!(!out.tool_runs[1].ok);
        let seen = script.seen.lock().unwrap();
        let Msg::ToolResults(rs) = seen[1].last().unwrap() else { panic!() };
        assert_eq!(rs.len(), 3, "one result per call id");
        assert_eq!(rs[0].content, rs[1].content);
        assert!(rs[2].is_error);
    }

    #[test]
    fn cancel_stops_before_sending() {
        let c = ctx();
        let flag = AtomicBool::new(true);
        let script = Script::new(vec![text("never")]);
        let err = rt().block_on(run(&script, &c, "s", vec![], "q", &flag, &mut |_| {})).unwrap_err();
        assert!(err.downcast_ref::<Cancelled>().is_some());
        assert!(script.seen.lock().unwrap().is_empty());
    }

    #[test]
    fn fit_elides_old_results_then_history() {
        let big = "x".repeat(20_000);
        let mut messages = vec![
            Msg::User("old question".into()),
            Msg::Assistant { text: "y".repeat(20_000), calls: vec![], raw: None },
            Msg::User("new question".into()),
            Msg::Assistant { text: String::new(), calls: vec![], raw: None },
            Msg::ToolResults(vec![ToolResult { call_id: "1".into(), content: big.clone(), is_error: false }]),
            Msg::Assistant { text: String::new(), calls: vec![], raw: None },
            Msg::ToolResults(vec![ToolResult { call_id: "2".into(), content: big.clone(), is_error: false }]),
        ];
        let mut history_len = 2;
        // Limit that fits after dropping one tool result.
        let total = estimate_request_tokens(&ChatRequest { system: "s", messages: &messages, tools: &[], max_tokens: 0 });
        fit_to_budget("s", &[], &mut messages, &mut history_len, total - 3000).unwrap();
        let Msg::ToolResults(first) = &messages[4] else { panic!() };
        assert_eq!(first[0].content, ELIDED, "older result goes first");
        assert_eq!(history_len, 2, "history kept while results can shrink");
        // Tighter: history goes next, the latest result last.
        fit_to_budget("s", &[], &mut messages, &mut history_len, total - 9000).unwrap();
        assert_eq!(history_len, 0);
        assert!(matches!(&messages[0], Msg::User(q) if q == "new question"));
        let Msg::ToolResults(last) = messages.last().unwrap() else { panic!() };
        assert_eq!(last[0].content, big);
        // Impossible: the question alone is over the limit.
        let mut huge = vec![Msg::User("z".repeat(400_000))];
        let mut h = 0;
        let err = fit_to_budget("s", &[], &mut huge, &mut h, MAX_INPUT_TOKENS).unwrap_err();
        assert!(err.to_string().contains("too large"));
    }

    #[test]
    fn caps_tool_results() {
        let s = cap_result("é".repeat(RESULT_MAX_CHARS));
        assert!(s.len() < RESULT_MAX_CHARS + 200);
        assert!(s.ends_with("or a limit.]"));
        assert_eq!(cap_result("short".into()), "short");
    }

    #[test]
    fn citations_are_checked() {
        let text = "You reviewed [the PR](card:12) and then [scrolled](card:99), see [again](card://12).";
        assert_eq!(cited_ids(text), vec![12, 99]);
        let allowed: HashSet<i64> = [12, 99].into_iter().collect();
        let (clean, kept) = sanitize_citations(text, &allowed, |id| id == 12);
        assert_eq!(clean, "You reviewed [the PR](card:12) and then scrolled, see [again](card:12).");
        assert_eq!(kept, vec![12]);
        let (clean, kept) = sanitize_citations("[x](card:12)", &HashSet::new(), |_| true);
        assert_eq!((clean.as_str(), kept.len()), ("x", 0));
    }
}
