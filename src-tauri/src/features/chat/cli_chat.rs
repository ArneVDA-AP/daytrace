//! Chat through the Claude Code CLI (`claude -p`), on the user's Claude subscription.
//!
//! Dayflow's CLI chat (Core/AI/ChatCLIProcessRunner.swift, ChatService.buildClaudePrompt; MIT,
//! (c) 2025 Jerry Liu) lets the agent query the database with sqlite3 in a shell. Daytrace gives
//! the CLI no shell and no files: its only tools are daytrace-mcp's read tools, started with
//! `--chat` through `--mcp-config` (llm/cli/args.rs `claude_chat_args` lists every flag). The
//! system prompt is the chat's own with those tools named (prompt.rs `ToolSet::Mcp`); earlier
//! turns go into the question the way Dayflow builds its CLI prompt ("User: ..." /
//! "Assistant: ..."), so nothing is kept in a Claude Code session.
//!
//! The `stream-json` events become the chat's progress steps: a `tool_use` block is a step
//! starting, its `tool_result` the step finishing, and the `result` event the answer. Card ids for
//! citations are the `record_id` values in the tool results.

use super::engine::{Cancelled, Outcome, Progress, ToolRun, MAX_TOOL_ROUNDS, ROUNDS_EXCEEDED};
use crate::features::agent_bridge::mcp::READ_TOOLS;
use crate::llm::cli::args::{claude_chat_args, ChatArgs};
use crate::llm::cli::output::{extract_cli_error, failure_message, strip_ansi};
use crate::llm::cli::{CliBackend, CliFailed, CliTool};
use anyhow::{anyhow, bail, Result};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// The MCP server's name in the chat's config; tool names are `mcp__daytrace__<tool>`.
pub const SERVER: &str = "daytrace";
/// Reasoning effort for chat questions (the app's default reasoning is "low").
pub const EFFORT: &str = "low";
/// Earlier turns sent with a question, as in the API chat.
pub const MAX_HISTORY_MESSAGES: usize = 40;
/// Earlier turns are cut (oldest first) to stay under this many characters.
pub const MAX_HISTORY_CHARS: usize = 60_000;

pub struct CliChat<'a> {
    pub backend: &'a CliBackend,
    /// daytrace-mcp next to the app.
    pub mcp_exe: PathBuf,
    /// The app's data folder, passed to daytrace-mcp as DAYTRACE_DATA_DIR.
    pub data_dir: PathBuf,
    pub timeout: Duration,
}

/// Full names of the tools the CLI may call.
pub fn allowed_tools() -> Vec<String> {
    READ_TOOLS.iter().map(|t| format!("mcp__{SERVER}__{t}")).collect()
}

/// The `--mcp-config` file contents.
pub fn mcp_config(mcp_exe: &std::path::Path, data_dir: &std::path::Path) -> Value {
    json!({"mcpServers": {SERVER: {
        "type": "stdio",
        "command": mcp_exe.to_string_lossy(),
        "args": ["--chat"],
        "env": {"DAYTRACE_DATA_DIR": data_dir.to_string_lossy()},
    }}})
}

/// The text on stdin: earlier turns (newest kept) and the question. `history` holds (role, text)
/// with role "user" or "assistant".
pub fn prompt(history: &[(String, String)], question: &str) -> String {
    let mut turns: Vec<String> = Vec::new();
    let mut size = 0;
    for (role, text) in history.iter().rev().take(MAX_HISTORY_MESSAGES) {
        let label = if role == "user" { "User" } else { "Assistant" };
        let t = format!("{label}: {}", text.trim());
        size += t.len();
        if size > MAX_HISTORY_CHARS {
            break;
        }
        turns.push(t);
    }
    turns.reverse();
    // Start with a question, as the API chat does.
    while turns.first().is_some_and(|t| t.starts_with("Assistant: ")) {
        turns.remove(0);
    }
    if turns.is_empty() {
        return question.trim().to_string();
    }
    format!("Earlier in this conversation:\n\n{}\n\nNew question from the user:\n\n{}", turns.join("\n\n"), question.trim())
}

/// A tool step's label, like the API chat's ("Fetching timeline for Thu, Oct 15").
pub fn label(tool: &str, input: &Value) -> String {
    let range = || {
        let s = |k: &str| input.get(k).and_then(Value::as_str).map(str::trim).filter(|v| !v.is_empty()).map(String::from);
        let args = match (s("date"), s("from"), s("to")) {
            (Some(d), _, _) => json!({"date": d}),
            (None, Some(f), Some(t)) => json!({"startDate": f, "endDate": t}),
            _ => return None,
        };
        super::tools::parse_range(&args).ok().map(|r| super::tools::describe_range(&r))
    };
    let with = |base: &str, today: &str| match range() {
        Some(r) => format!("{base} for {r}"),
        None => today.to_string(),
    };
    match tool {
        "get_timeline" => with("Fetching timeline", "Fetching today's timeline"),
        "get_time_breakdown" => with("Adding up time per category", "Adding up time per category"),
        "get_activity_detail" => "Opening an activity".into(),
        "get_observations" => "Reading an activity's screen observations".into(),
        "search_activities" => match input.get("query").and_then(Value::as_str).map(str::trim).filter(|q| !q.is_empty()) {
            Some(q) => format!("Searching for “{q}”"),
            None => "Searching activities".into(),
        },
        "get_daily" => with("Reading the daily recap", "Reading today's daily recap"),
        "get_weekly" => with("Adding up the week", "Adding up this week"),
        "list_days" => "Listing days with activities".into(),
        "list_categories" => "Reading the categories".into(),
        "get_status" => "Checking recording and analysis".into(),
        _ => "Running a tool".into(),
    }
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

/// One line about a tool result.
pub fn summarize(text: &str, is_error: bool) -> String {
    let v: Value = serde_json::from_str(text.trim()).unwrap_or(Value::Null);
    if is_error || v.get("error").is_some() {
        let m = v.get("error").and_then(Value::as_str).unwrap_or(text).trim().to_string();
        return crate::llm::client::truncate(&m, 160);
    }
    let count = |k: &str| v.get(k).and_then(Value::as_array).map(Vec::len);
    if let Some(n) = count("cards") {
        return if n == 0 { "No activities".into() } else { plural(n, "activity", "activities") };
    }
    if let Some(n) = count("matches") {
        return if n == 0 { "No matches".into() } else { plural(n, "match", "matches") };
    }
    if let Some(n) = count("observations") {
        return if n == 0 { "No observations".into() } else { plural(n, "observation", "observations") };
    }
    if let Some(n) = count("days") {
        return plural(n, "day", "days");
    }
    if v.get("highlights").is_some() {
        return "Daily recap".into();
    }
    if let Some(m) = v.get("tracked_minutes").and_then(Value::as_i64) {
        return format!("{} tracked", super::tools::fmt_minutes(m));
    }
    if let Some(n) = count("categories") {
        return plural(n, "category", "categories");
    }
    if v.get("record_id").is_some() {
        return "1 activity".into();
    }
    if v.get("today").is_some() {
        return "Status".into();
    }
    "Done".into()
}

/// Every `record_id` in a tool result.
pub fn record_ids(text: &str, out: &mut HashSet<i64>) {
    fn walk(v: &Value, out: &mut HashSet<i64>) {
        match v {
            Value::Object(m) => {
                for (k, x) in m {
                    if k == "record_id" {
                        if let Some(id) = x.as_i64() {
                            out.insert(id);
                        }
                    } else {
                        walk(x, out);
                    }
                }
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
        walk(&v, out);
    }
}

/// The text of a tool_result's `content` (a string or a list of text blocks).
fn result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(a) => a.iter().filter_map(|b| b.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// What the stream said so far.
#[derive(Default)]
pub struct Stream {
    pending: HashMap<String, (String, String, Value)>,
    pub tool_runs: Vec<ToolRun>,
    pub seen_card_ids: HashSet<i64>,
    /// The last assistant text (an answer written before a final tool call).
    pub last_text: String,
    pub result: Option<Value>,
    /// daytrace-mcp did not start.
    pub mcp_failed: Option<String>,
}

impl Stream {
    /// Handles one stdout line; returns the progress steps it produced.
    pub fn line(&mut self, line: &str) -> Vec<Progress> {
        let mut steps = Vec::new();
        let Ok(ev) = serde_json::from_str::<Value>(strip_ansi(line).trim()) else { return steps };
        match ev.get("type").and_then(Value::as_str) {
            Some("system") if ev.get("subtype").and_then(Value::as_str) == Some("init") => {
                let ours = ev.get("mcp_servers").and_then(Value::as_array).and_then(|a| a.iter().find(|s| s.get("name").and_then(Value::as_str) == Some(SERVER)));
                match ours.and_then(|s| s.get("status")).and_then(Value::as_str) {
                    Some("connected") | Some("pending") | None => {}
                    Some(other) => self.mcp_failed = Some(other.to_string()),
                }
                if ours.is_none() && ev.get("mcp_servers").is_some() {
                    self.mcp_failed = Some("missing".into());
                }
            }
            Some("assistant") if ev.get("parent_tool_use_id").is_none_or(Value::is_null) => {
                for block in ev.pointer("/message/content").and_then(Value::as_array).into_iter().flatten() {
                    match block.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            let t = block.get("text").and_then(Value::as_str).unwrap_or("").trim();
                            if !t.is_empty() {
                                self.last_text = t.to_string();
                            }
                        }
                        Some("tool_use") => {
                            let id = block.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                            let full = block.get("name").and_then(Value::as_str).unwrap_or("");
                            let name = full.strip_prefix(&format!("mcp__{SERVER}__")).unwrap_or(full).to_string();
                            let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                            let l = label(&name, &input);
                            steps.push(Progress::ToolStarted { name: name.clone(), label: l.clone() });
                            self.pending.insert(id, (name, l, input));
                        }
                        _ => {}
                    }
                }
            }
            Some("user") => {
                let mut finished = false;
                for block in ev.pointer("/message/content").and_then(Value::as_array).into_iter().flatten() {
                    if block.get("type").and_then(Value::as_str) != Some("tool_result") {
                        continue;
                    }
                    let id = block.get("tool_use_id").and_then(Value::as_str).unwrap_or("");
                    let Some((name, label, input)) = self.pending.remove(id) else { continue };
                    let text = result_text(block.get("content").unwrap_or(&Value::Null));
                    let is_error = block.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                    let summary = summarize(&text, is_error);
                    let ok = !is_error && !summary.is_empty() && serde_json::from_str::<Value>(text.trim()).map(|v| v.get("error").is_none()).unwrap_or(false);
                    if ok {
                        record_ids(&text, &mut self.seen_card_ids);
                    }
                    steps.push(Progress::ToolFinished { name: name.clone(), summary: summary.clone(), ok });
                    self.tool_runs.push(ToolRun { name, label, input, summary, ok });
                    finished = true;
                }
                if finished {
                    steps.push(Progress::Thinking);
                }
            }
            Some("result") => self.result = Some(ev),
            _ => {}
        }
        steps
    }

    /// The answer, or why there is none.
    pub fn finish(self, code: Option<i32>, stdout: &str, stderr: &str) -> Result<Outcome> {
        let Some(r) = self.result else {
            let message = extract_cli_error(stdout, stderr).unwrap_or_else(|| failure_message("claude", code, "", stderr));
            return Err(CliFailed { tool: CliTool::Claude, message, usage: None }.into());
        };
        let usage = r.get("usage");
        let n = |k: &str| usage.and_then(|u| u.get(k)).and_then(Value::as_i64).unwrap_or(0);
        let input_tokens = n("input_tokens") + n("cache_read_input_tokens") + n("cache_creation_input_tokens");
        let output_tokens = n("output_tokens");
        let subtype = r.get("subtype").and_then(Value::as_str).unwrap_or("");
        let text = r.get("result").and_then(Value::as_str).unwrap_or("").trim().to_string();
        if subtype == "error_max_turns" {
            bail!(ROUNDS_EXCEEDED);
        }
        if r.get("is_error").and_then(Value::as_bool).unwrap_or(false) || (subtype != "success" && !subtype.is_empty()) {
            let message = if text.is_empty() { format!("Claude Code stopped ({subtype}).") } else { text };
            return Err(CliFailed { tool: CliTool::Claude, message, usage: None }.into());
        }
        let text = if text.is_empty() { self.last_text } else { text };
        if text.is_empty() {
            bail!("The model returned an empty answer.");
        }
        Ok(Outcome {
            text,
            tool_runs: self.tool_runs,
            seen_card_ids: self.seen_card_ids,
            input_tokens,
            output_tokens,
            requests: r.get("num_turns").and_then(Value::as_u64).unwrap_or(1) as usize,
        })
    }
}

/// Runs one question through the CLI.
pub async fn run(
    chat: &CliChat<'_>,
    system: &str,
    history: &[(String, String)],
    question: &str,
    cancel: &AtomicBool,
    on_progress: &mut (dyn FnMut(Progress) + Send),
) -> Result<Outcome> {
    if !chat.mcp_exe.is_file() {
        bail!("daytrace-mcp isn't next to Daytrace ({}), so the Claude Code CLI can't read your timeline. Install Daytrace again to get it.", chat.mcp_exe.display());
    }
    let folder = chat.backend.temp_dir("chat")?;
    let system_file = folder.path().join("system-prompt.md");
    let mcp_file = folder.path().join("mcp.json");
    std::fs::write(&system_file, system)?;
    std::fs::write(&mcp_file, serde_json::to_string_pretty(&mcp_config(&chat.mcp_exe, &chat.data_dir))?)?;
    let tools = allowed_tools();
    let args = claude_chat_args(&ChatArgs {
        model: &chat.backend.model,
        effort: EFFORT,
        system_prompt_file: &system_file,
        mcp_config_file: &mcp_file,
        allowed_tools: &tools,
        // Tool rounds plus the answer.
        max_turns: MAX_TOOL_ROUNDS + 1,
    });

    on_progress(Progress::Thinking);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let running = chat.backend.run_streaming(args, prompt(history, question), chat.timeout, tx);
    tokio::pin!(running);
    let mut stream = Stream::default();
    let output = loop {
        tokio::select! {
            Some(line) = rx.recv() => {
                for step in stream.line(&line) {
                    on_progress(step);
                }
                if let Some(status) = &stream.mcp_failed {
                    // Dropping `running` ends the CLI and everything it started.
                    bail!("The Claude Code CLI couldn't start Daytrace's MCP server ({status}). Check that daytrace-mcp is next to Daytrace, then try again.");
                }
            }
            out = &mut running => break out?,
            _ = wait_cancel(cancel) => return Err(Cancelled.into()),
        }
    };
    while let Ok(line) = rx.try_recv() {
        for step in stream.line(&line) {
            on_progress(step);
        }
    }
    drop(folder);
    stream.finish(output.code, &output.stdout, &output.stderr).map_err(|e| {
        if e.downcast_ref::<CliFailed>().is_none() && e.to_string().is_empty() {
            anyhow!("Claude Code ended without an answer.")
        } else {
            e
        }
    })
}

async fn wait_cancel(flag: &AtomicBool) {
    while !flag.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::cli::discover::SysEnv;
    use crate::llm::cli::process::FakeRunner;
    use std::sync::Arc;

    // Lines in the shape `claude -p --output-format stream-json --verbose` prints (Agent SDK
    // message types: system/init, assistant, user, result).
    fn transcript(card: i64) -> String {
        [
            json!({"type": "system", "subtype": "init", "session_id": "s", "tools": ["mcp__daytrace__get_timeline"], "mcp_servers": [{"name": "daytrace", "status": "connected"}]}),
            json!({"type": "assistant", "parent_tool_use_id": null, "message": {"content": [
                {"type": "text", "text": "Let me check."},
                {"type": "tool_use", "id": "t1", "name": "mcp__daytrace__get_timeline", "input": {"date": "2026-10-15"}},
            ]}}),
            json!({"type": "user", "parent_tool_use_id": null, "message": {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "t1", "content": [{"type": "text", "text": format!(r#"{{"cards":[{{"record_id":{card},"title":"Parser"}}],"date":"2026-10-15","schema_version":1}}"#)}]},
            ]}}),
            json!({"type": "assistant", "parent_tool_use_id": null, "message": {"content": [
                {"type": "tool_use", "id": "t2", "name": "mcp__daytrace__search_activities", "input": {"query": "pricing"}},
            ]}}),
            json!({"type": "user", "message": {"content": [
                {"type": "tool_result", "tool_use_id": "t2", "is_error": true, "content": r#"{"error":"query is required.","schema_version":1}"#},
            ]}}),
            json!({"type": "result", "subtype": "success", "is_error": false, "num_turns": 3, "result": format!("You wrote [the parser](card:{card})."),
                "usage": {"input_tokens": 10, "cache_read_input_tokens": 1000, "cache_creation_input_tokens": 20, "output_tokens": 50}}),
        ]
        .iter()
        .map(|v| v.to_string())
        .collect::<Vec<_>>()
        .join("\n")
    }

    #[test]
    fn stream_events_become_progress_and_an_answer() {
        let mut s = Stream::default();
        let mut steps = Vec::new();
        for l in transcript(42).lines() {
            steps.extend(s.line(l));
        }
        let kinds: Vec<String> = steps.iter().map(|p| serde_json::to_value(p).unwrap()["stage"].as_str().unwrap().to_string()).collect();
        assert_eq!(kinds, ["toolStarted", "toolFinished", "thinking", "toolStarted", "toolFinished", "thinking"]);
        let out = s.finish(Some(0), "", "").unwrap();
        assert_eq!(out.text, "You wrote [the parser](card:42).");
        assert_eq!(out.seen_card_ids, HashSet::from([42]));
        assert_eq!((out.input_tokens, out.output_tokens, out.requests), (1030, 50, 3));
        assert_eq!(out.tool_runs.len(), 2);
        assert_eq!(out.tool_runs[0].name, "get_timeline");
        assert_eq!(out.tool_runs[0].label, "Fetching timeline for Thu, Oct 15");
        assert_eq!((out.tool_runs[0].summary.as_str(), out.tool_runs[0].ok), ("1 activity", true));
        assert_eq!((out.tool_runs[1].summary.as_str(), out.tool_runs[1].ok), ("query is required.", false));
    }

    #[test]
    fn failures_in_plain_terms() {
        // Signed out: Claude Code prints the reason as the result.
        let mut s = Stream::default();
        s.line(r#"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in · Please run /login"}"#);
        let e = s.finish(Some(1), "", "").unwrap_err();
        assert_eq!(e.downcast_ref::<CliFailed>().unwrap().message, "Not logged in · Please run /login");
        let mut s = Stream::default();
        s.line(r#"{"type":"result","subtype":"error_max_turns","is_error":true,"num_turns":9}"#);
        assert_eq!(s.finish(Some(1), "", "").unwrap_err().to_string(), ROUNDS_EXCEEDED);
        // No result event at all.
        let e = Stream::default().finish(Some(1), "", "error: unknown option '--permission-mode'").unwrap_err();
        assert!(e.downcast_ref::<CliFailed>().unwrap().message.contains("unknown option"));
        // The MCP server did not start.
        let mut s = Stream::default();
        s.line(r#"{"type":"system","subtype":"init","mcp_servers":[{"name":"daytrace","status":"failed"}]}"#);
        assert_eq!(s.mcp_failed.as_deref(), Some("failed"));
        // An answer written before a last tool call still counts.
        let mut s = Stream::default();
        s.line(r#"{"type":"assistant","message":{"content":[{"type":"text","text":"Mostly coding."}]}}"#);
        s.line(r#"{"type":"result","subtype":"success","is_error":false,"result":""}"#);
        assert_eq!(s.finish(Some(0), "", "").unwrap().text, "Mostly coding.");
    }

    #[test]
    fn prompt_carries_earlier_turns() {
        assert_eq!(prompt(&[], " What did I do? "), "What did I do?");
        let h = vec![
            ("assistant".to_string(), "orphan".to_string()),
            ("user".to_string(), "q1".to_string()),
            ("assistant".to_string(), "a1".to_string()),
        ];
        assert_eq!(prompt(&h, "q2"), "Earlier in this conversation:\n\nUser: q1\n\nAssistant: a1\n\nNew question from the user:\n\nq2");
        let long: Vec<(String, String)> = (0..100).map(|i| (if i % 2 == 0 { "user" } else { "assistant" }.to_string(), "x".repeat(2000))).collect();
        let p = prompt(&long, "q");
        assert!(p.len() < MAX_HISTORY_CHARS + 200);
        assert!(p.contains("Earlier in this conversation:\n\nUser: "));
    }

    #[test]
    fn labels_summaries_and_ids() {
        assert_eq!(label("get_timeline", &json!({})), "Fetching today's timeline");
        assert_eq!(label("get_time_breakdown", &json!({"from": "2026-10-12", "to": "2026-10-16"})), "Adding up time per category for Oct 12 to Oct 16");
        assert_eq!(label("search_activities", &json!({"query": "pricing page"})), "Searching for “pricing page”");
        assert_eq!(summarize(r#"{"matches":[]}"#, false), "No matches");
        assert_eq!(summarize(r#"{"days":[1,2]}"#, false), "2 days");
        assert_eq!(summarize(r#"{"tracked_minutes":95,"categories":[]}"#, false), "1 h 35 min tracked");
        assert_eq!(summarize("not json", true), "not json");
        let mut ids = HashSet::new();
        record_ids(r#"{"matches":[{"record_id":3},{"record_id":4,"x":{"record_id":5}}],"record_id":"7"}"#, &mut ids);
        assert_eq!(ids, HashSet::from([3, 4, 5]));
        assert_eq!(allowed_tools()[0], "mcp__daytrace__get_time_breakdown");
        let cfg = mcp_config(std::path::Path::new(r"C:\D\daytrace-mcp.exe"), std::path::Path::new(r"C:\Data"));
        assert_eq!(cfg["mcpServers"]["daytrace"]["args"], json!(["--chat"]));
        assert_eq!(cfg["mcpServers"]["daytrace"]["env"]["DAYTRACE_DATA_DIR"], r"C:\Data");
    }

    struct NoEnv;
    impl SysEnv for NoEnv {
        fn var(&self, _: &str) -> Option<String> {
            None
        }
        fn is_file(&self, p: &str) -> bool {
            std::path::Path::new(p).is_file()
        }
        fn is_dir(&self, p: &str) -> bool {
            std::path::Path::new(p).is_dir()
        }
    }

    #[tokio::test]
    async fn runs_the_cli_with_the_chat_flags() {
        let dir = tempfile::tempdir().unwrap();
        let claude = dir.path().join("claude");
        let mcp = dir.path().join("daytrace-mcp");
        std::fs::write(&claude, b"").unwrap();
        std::fs::write(&mcp, b"").unwrap();
        let work = dir.path().join("work");
        let seen = Arc::new(std::sync::Mutex::new(None::<(String, String)>));
        let s2 = seen.clone();
        let mut runner = FakeRunner::new(vec![FakeRunner::ok(&transcript(7))]);
        // The files exist while the CLI runs.
        runner.on_run = Some(Box::new(move |spec| {
            let file = |flag: &str| spec.args.iter().position(|a| a == flag).map(|i| std::fs::read_to_string(&spec.args[i + 1]).unwrap()).unwrap();
            *s2.lock().unwrap() = Some((file("--system-prompt-file"), file("--mcp-config")));
        }));
        let runner = Arc::new(runner);
        let backend = CliBackend::with_parts(CliTool::Claude, &claude.to_string_lossy(), "haiku", runner.clone(), Arc::new(NoEnv), work.clone(), false);
        let chat = CliChat { backend: &backend, mcp_exe: mcp.clone(), data_dir: dir.path().join("data"), timeout: Duration::from_secs(30) };
        let cancel = AtomicBool::new(false);
        let mut steps = Vec::new();
        let out = run(&chat, "SYSTEM", &[("user".into(), "q1".into()), ("assistant".into(), "a1".into())], "q2", &cancel, &mut |p| steps.push(p)).await.unwrap();
        assert_eq!(out.text, "You wrote [the parser](card:7).");
        assert_eq!(steps.len(), 7, "thinking first, then the stream's steps");
        let spec = &runner.calls()[0];
        assert_eq!(spec.args[..4], ["-p", "--output-format", "stream-json", "--verbose"]);
        assert!(spec.args.windows(2).any(|w| w[0] == "--model" && w[1] == "haiku"));
        assert!(spec.args.windows(2).any(|w| w[0] == "--tools" && w[1].is_empty()));
        assert!(String::from_utf8(spec.stdin.clone()).unwrap().ends_with("New question from the user:\n\nq2"));
        let (system, cfg) = seen.lock().unwrap().clone().unwrap();
        assert_eq!(system, "SYSTEM");
        assert_eq!(serde_json::from_str::<Value>(&cfg).unwrap()["mcpServers"]["daytrace"]["command"], mcp.to_string_lossy().as_ref());
        // The run's folder is gone afterwards.
        assert_eq!(std::fs::read_dir(&work).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().starts_with("chat-")).count(), 0);

        // Without daytrace-mcp there is nothing to run.
        let chat = CliChat { backend: &backend, mcp_exe: dir.path().join("missing"), data_dir: dir.path().into(), timeout: Duration::from_secs(5) };
        assert!(run(&chat, "S", &[], "q", &cancel, &mut |_| {}).await.unwrap_err().to_string().contains("isn't next to Daytrace"));
    }
}
