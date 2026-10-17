//! Chat through the Codex CLI (`codex exec`), on the user's ChatGPT account.
//!
//! Dayflow's Codex chat (Core/AI/ChatCLIProcessRunner.swift executeStreaming and parseCodexEvent,
//! MIT, (c) 2025 Jerry Liu) runs `codex exec --json` and lets the agent query the database with
//! sqlite3 in a shell. Daytrace gives Codex no shell and no writes: its only tools are
//! daytrace-mcp's read tools, started with `--chat` as the run's one MCP server
//! (llm/cli/args.rs `codex_chat_args` lists every flag and why). Codex has no system prompt flag,
//! so the chat prompt (prompt.rs `ToolSet::Mcp`) goes at the top of the text on stdin, followed by
//! the earlier turns and the question as for Claude Code (cli_chat.rs `prompt`).
//!
//! The JSONL events become the chat's progress steps: an `mcp_tool_call` item starting is a step
//! starting, the item completing is the step finishing, the last `agent_message` is the answer and
//! `turn.completed` carries the token counts. Card ids for citations are the `record_id` values in
//! the tool results. As a second line of defense, a run that shows a command, a file change, a web
//! search or another MCP server is stopped at once.

use super::cli_chat::{label, record_ids, summarize, wait_cancel, SERVER};
use super::engine::{Cancelled, Outcome, Progress, ToolRun, MAX_TOOL_ROUNDS, ROUNDS_EXCEEDED};
use crate::features::agent_bridge::mcp::READ_TOOLS;
use crate::llm::cli::args::{codex_chat_args, CodexChatArgs};
use crate::llm::cli::output::{extract_cli_error, failure_message, strip_ansi};
use crate::llm::cli::{CliBackend, CliFailed, CliTool};
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// Reasoning effort for chat questions (`model_reasoning_effort`).
pub const EFFORT: &str = "low";

pub struct CodexChat<'a> {
    pub backend: &'a CliBackend,
    /// daytrace-mcp next to the app.
    pub mcp_exe: PathBuf,
    /// The app's data folder, passed to daytrace-mcp as DAYTRACE_DATA_DIR.
    pub data_dir: PathBuf,
    pub timeout: Duration,
}

/// The text on stdin: the chat's instructions, then the earlier turns and the question.
pub fn prompt(system: &str, history: &[(String, String)], question: &str) -> String {
    format!("{}\n\n---\n\n{}", system.trim(), super::cli_chat::prompt(history, question))
}

/// Why a run was stopped: Codex did something the chat never allows.
#[derive(Debug, PartialEq)]
pub enum Violation {
    Command,
    FileChange,
    WebSearch,
    OtherServer(String),
}

impl Violation {
    fn message(&self) -> String {
        let what = match self {
            Violation::Command => "tried to run a command".to_string(),
            Violation::FileChange => "tried to change a file".to_string(),
            Violation::WebSearch => "tried to search the web".to_string(),
            Violation::OtherServer(s) => format!("tried to use another tool server ({s})"),
        };
        format!("The Codex CLI {what}, which Daytrace's chat doesn't allow, so Daytrace stopped it. Update the Codex CLI (npm install -g @openai/codex@latest) and try again.")
    }
}

/// The text of an MCP result's content blocks.
fn content_text(result: &Value) -> String {
    match result.get("content") {
        Some(Value::Array(a)) => a.iter().filter_map(|b| b.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join("\n"),
        Some(Value::String(s)) => s.clone(),
        _ => result.get("structured_content").filter(|v| !v.is_null()).map(|v| v.to_string()).unwrap_or_default(),
    }
}

/// An error message that is itself a JSON event (Dayflow: "API failures can themselves be JSON
/// encoded inside the JSONL message").
fn unwrap_message(m: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(m.trim()) {
        let nested = v.pointer("/error/message").or_else(|| v.get("message")).and_then(Value::as_str);
        if let Some(n) = nested.filter(|n| !n.trim().is_empty()) {
            return n.trim().to_string();
        }
    }
    m.trim().to_string()
}

/// What the stream said so far.
#[derive(Default)]
pub struct Stream {
    pending: HashMap<String, (String, String, Value)>,
    /// Calls already finished (an item.completed can repeat what came before).
    done_ids: HashSet<String>,
    pub tool_runs: Vec<ToolRun>,
    pub seen_card_ids: HashSet<i64>,
    /// The last agent message: the answer once the turn completes.
    pub last_text: String,
    pub usage: Option<Value>,
    pub completed: bool,
    pub failed: Option<String>,
    /// The last `error` event (Codex retries after some; the turn may still complete).
    pub last_error: Option<String>,
    pub violation: Option<Violation>,
    /// Tool rounds: tool calls that start while none is running.
    pub rounds: usize,
}

impl Stream {
    /// Handles one stdout line; returns the progress steps it produced.
    pub fn line(&mut self, line: &str) -> Vec<Progress> {
        let mut steps = Vec::new();
        let Ok(ev) = serde_json::from_str::<Value>(strip_ansi(line).trim()) else { return steps };
        let kind = ev.get("type").and_then(Value::as_str).unwrap_or("");
        match kind {
            "turn.completed" => {
                self.completed = true;
                self.usage = ev.get("usage").cloned();
            }
            "turn.failed" => {
                let m = ev.pointer("/error/message").and_then(Value::as_str).unwrap_or("Codex could not complete the request.");
                self.failed = Some(unwrap_message(m));
            }
            "error" => {
                if let Some(m) = ev.get("message").and_then(Value::as_str) {
                    self.last_error = Some(unwrap_message(m));
                }
            }
            "item.started" | "item.updated" | "item.completed" => {
                let Some(item) = ev.get("item") else { return steps };
                let started = kind == "item.started";
                let done = kind == "item.completed";
                match item.get("type").and_then(Value::as_str).unwrap_or("") {
                    "command_execution" => self.violation = Some(Violation::Command),
                    "file_change" => self.violation = Some(Violation::FileChange),
                    "web_search" => self.violation = Some(Violation::WebSearch),
                    "mcp_tool_call" => {
                        let server = item.get("server").and_then(Value::as_str).unwrap_or("");
                        if server != SERVER {
                            self.violation = Some(Violation::OtherServer(server.to_string()));
                            return steps;
                        }
                        let id = item.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                        let name = item.get("tool").and_then(Value::as_str).unwrap_or("").to_string();
                        let input = item.get("arguments").cloned().filter(|v| v.is_object()).unwrap_or_else(|| json!({}));
                        if (started || done) && !self.pending.contains_key(&id) && !self.done_ids.contains(&id) {
                            if self.pending.is_empty() {
                                self.rounds += 1;
                            }
                            let l = label(&name, &input);
                            steps.push(Progress::ToolStarted { name: name.clone(), label: l.clone() });
                            self.pending.insert(id.clone(), (name, l, input));
                        }
                        if done {
                            let Some((name, label, input)) = self.pending.remove(&id) else { return steps };
                            let status = item.get("status").and_then(Value::as_str).unwrap_or("");
                            let error = item.pointer("/error/message").and_then(Value::as_str).map(str::to_string);
                            let text = item.get("result").filter(|r| !r.is_null()).map(content_text).unwrap_or_default();
                            let is_error = status == "failed" || error.is_some();
                            let summary = match &error {
                                Some(e) if text.trim().is_empty() => crate::llm::client::truncate(e.trim(), 160),
                                _ => summarize(&text, is_error),
                            };
                            let ok = !is_error && serde_json::from_str::<Value>(text.trim()).map(|v| v.get("error").is_none()).unwrap_or(false);
                            if ok {
                                record_ids(&text, &mut self.seen_card_ids);
                            }
                            steps.push(Progress::ToolFinished { name: name.clone(), summary: summary.clone(), ok });
                            self.tool_runs.push(ToolRun { name, label, input, summary, ok });
                            self.done_ids.insert(id);
                            if self.pending.is_empty() {
                                steps.push(Progress::Thinking);
                            }
                        }
                    }
                    "agent_message" if done => {
                        let t = item.get("text").and_then(Value::as_str).unwrap_or("").trim();
                        if !t.is_empty() {
                            self.last_text = t.to_string();
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        steps
    }

    /// The answer, or why there is none.
    pub fn finish(self, code: Option<i32>, stdout: &str, stderr: &str) -> Result<Outcome> {
        if let Some(v) = &self.violation {
            bail!(v.message());
        }
        if let Some(m) = self.failed {
            return Err(CliFailed { tool: CliTool::Codex, message: m, usage: None }.into());
        }
        if !self.completed {
            let message = self.last_error.clone().or_else(|| extract_cli_error(stdout, stderr)).unwrap_or_else(|| failure_message("codex", code, "", stderr));
            return Err(CliFailed { tool: CliTool::Codex, message: plain_failure(&message), usage: None }.into());
        }
        if self.last_text.is_empty() {
            bail!("The model returned an empty answer.");
        }
        let n = |k: &str| self.usage.as_ref().and_then(|u| u.get(k)).and_then(Value::as_i64).unwrap_or(0);
        Ok(Outcome {
            text: self.last_text,
            tool_runs: self.tool_runs,
            seen_card_ids: self.seen_card_ids,
            // input_tokens already includes cached_input_tokens (OpenAI usage).
            input_tokens: n("input_tokens"),
            output_tokens: n("output_tokens"),
            requests: self.rounds + 1,
        })
    }
}

/// Failures that come before any event, in words the chat's error mapping understands.
fn plain_failure(message: &str) -> String {
    let lower = message.to_lowercase();
    if lower.contains("unexpected argument") || lower.contains("unrecognized subcommand") {
        // llm/failure.rs classifies this as CliOutdated.
        return format!("This chat needs a newer version of codex ({})", message.trim());
    }
    message.to_string()
}

/// Runs one question through the CLI.
pub async fn run(
    chat: &CodexChat<'_>,
    system: &str,
    history: &[(String, String)],
    question: &str,
    cancel: &AtomicBool,
    on_progress: &mut (dyn FnMut(Progress) + Send),
) -> Result<Outcome> {
    if !chat.mcp_exe.is_file() {
        bail!("daytrace-mcp isn't next to Daytrace ({}), so the Codex CLI can't read your timeline. Install Daytrace again to get it.", chat.mcp_exe.display());
    }
    let args = codex_chat_args(&CodexChatArgs {
        model: &chat.backend.model,
        effort: EFFORT,
        mcp_exe: &chat.mcp_exe,
        data_dir: &chat.data_dir,
        enabled_tools: READ_TOOLS,
    });

    on_progress(Progress::Thinking);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let running = chat.backend.run_streaming(args, prompt(system, history, question), chat.timeout, tx);
    tokio::pin!(running);
    let mut stream = Stream::default();
    // Dropping `running` (a return from this loop) ends the CLI and everything it started.
    let output = loop {
        tokio::select! {
            Some(line) = rx.recv() => {
                for step in stream.line(&line) {
                    on_progress(step);
                }
                if let Some(v) = &stream.violation {
                    log::warn!("codex chat stopped: {v:?}");
                    bail!(v.message());
                }
                if stream.rounds > MAX_TOOL_ROUNDS {
                    bail!(ROUNDS_EXCEEDED);
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
    let r = stream.finish(output.code, &output.stdout, &output.stderr);
    if let Err(e) = &r
        && let Some(c) = e.downcast_ref::<CliFailed>()
        && c.message.to_lowercase().contains("required mcp server")
    {
        bail!("The Codex CLI couldn't start Daytrace's MCP server ({}). Check that daytrace-mcp is next to Daytrace, then try again.", c.message);
    }
    r
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::cli::discover::SysEnv;
    use crate::llm::cli::process::FakeRunner;
    use std::sync::Arc;

    fn ev(v: Value) -> String {
        v.to_string()
    }

    /// Lines in the shape `codex exec --json` prints (openai/codex sdk/typescript/src/items.ts,
    /// events.ts: thread.started, turn.started, item.*, turn.completed).
    fn transcript(card: i64) -> String {
        [
            ev(json!({"type": "thread.started", "thread_id": "0199a213-81c0-7800-8aa1-bbab2a035a53"})),
            ev(json!({"type": "turn.started"})),
            ev(json!({"type": "item.completed", "item": {"id": "item_0", "type": "reasoning", "text": "Looking up the day."}})),
            ev(json!({"type": "item.started", "item": {"id": "item_1", "type": "mcp_tool_call", "server": "daytrace", "tool": "get_timeline", "arguments": {"date": "2026-10-15"}, "result": null, "error": null, "status": "in_progress"}})),
            ev(json!({"type": "item.completed", "item": {"id": "item_1", "type": "mcp_tool_call", "server": "daytrace", "tool": "get_timeline", "arguments": {"date": "2026-10-15"},
                "result": {"content": [{"type": "text", "text": format!(r#"{{"cards":[{{"record_id":{card},"title":"Parser"}}],"date":"2026-10-15","schema_version":1}}"#)}], "structured_content": null},
                "error": null, "status": "completed"}})),
            ev(json!({"type": "item.started", "item": {"id": "item_2", "type": "mcp_tool_call", "server": "daytrace", "tool": "search_activities", "arguments": {"query": "pricing"}, "result": null, "error": null, "status": "in_progress"}})),
            ev(json!({"type": "item.completed", "item": {"id": "item_2", "type": "mcp_tool_call", "server": "daytrace", "tool": "search_activities", "arguments": {"query": "pricing"}, "result": null, "error": {"message": "query is required."}, "status": "failed"}})),
            ev(json!({"type": "item.completed", "item": {"id": "item_3", "type": "agent_message", "text": format!("You wrote [the parser](card:{card}).")}})),
            ev(json!({"type": "turn.completed", "usage": {"input_tokens": 1030, "cached_input_tokens": 1000, "cache_write_input_tokens": 0, "output_tokens": 50, "reasoning_output_tokens": 20}})),
        ]
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
        assert_eq!(s.rounds, 2);
        let out = s.finish(Some(0), "", "").unwrap();
        assert_eq!(out.text, "You wrote [the parser](card:42).");
        assert_eq!(out.seen_card_ids, HashSet::from([42]));
        assert_eq!((out.input_tokens, out.output_tokens, out.requests), (1030, 50, 3));
        assert_eq!(out.tool_runs[0].label, "Fetching timeline for Thu, Oct 15");
        assert_eq!((out.tool_runs[0].summary.as_str(), out.tool_runs[0].ok), ("1 activity", true));
        assert_eq!((out.tool_runs[1].summary.as_str(), out.tool_runs[1].ok), ("query is required.", false));
    }

    #[test]
    fn parallel_calls_are_one_round_and_a_completed_item_alone_counts() {
        let mut s = Stream::default();
        let call = |id: &str, kind: &str| ev(json!({"type": kind, "item": {"id": id, "type": "mcp_tool_call", "server": "daytrace", "tool": "get_daily", "arguments": {}, "status": "completed",
            "result": {"content": [{"type": "text", "text": r#"{"highlights":[]}"#}], "structured_content": null}}}));
        let mut steps = s.line(&call("a", "item.started"));
        steps.extend(s.line(&call("b", "item.started")));
        steps.extend(s.line(&call("a", "item.completed")));
        steps.extend(s.line(&call("b", "item.completed")));
        // A call reported only when it completed.
        steps.extend(s.line(&call("c", "item.completed")));
        assert_eq!(s.rounds, 2);
        assert_eq!(s.tool_runs.len(), 3);
        assert_eq!(steps.iter().filter(|p| matches!(p, Progress::Thinking)).count(), 2);
        assert_eq!(s.tool_runs[2].summary, "Daily recap");
    }

    #[test]
    fn anything_but_daytrace_tools_stops_the_run() {
        for (item, v) in [
            (json!({"id": "x", "type": "command_execution", "command": "cmd /c dir", "aggregated_output": "", "exit_code": null, "status": "in_progress"}), Violation::Command),
            (json!({"id": "x", "type": "file_change", "changes": [{"path": "a.txt", "kind": "add"}], "status": "failed"}), Violation::FileChange),
            (json!({"id": "x", "type": "web_search", "query": "weather"}), Violation::WebSearch),
            (json!({"id": "x", "type": "mcp_tool_call", "server": "github", "tool": "list_repos", "arguments": {}, "status": "in_progress"}), Violation::OtherServer("github".into())),
        ] {
            let mut s = Stream::default();
            s.line(&ev(json!({"type": "item.started", "item": item})));
            assert_eq!(s.violation.as_ref(), Some(&v));
            assert!(s.finish(Some(0), "", "").unwrap_err().to_string().contains("which Daytrace's chat doesn't allow"));
        }
    }

    #[test]
    fn failures_in_plain_terms() {
        // Signed out: the turn fails with Codex's message (sometimes JSON inside the message).
        let mut s = Stream::default();
        s.line(&ev(json!({"type": "error", "message": "Reconnecting... 1/5"})));
        s.line(&ev(json!({"type": "turn.failed", "error": {"message": r#"{"type":"error","error":{"message":"Your access token could not be refreshed. Please log out and sign in again."}}"#}})));
        let e = s.finish(Some(1), "", "").unwrap_err();
        assert_eq!(e.downcast_ref::<CliFailed>().unwrap().message, "Your access token could not be refreshed. Please log out and sign in again.");
        // A retry error that the turn got past is not an error.
        let mut s = Stream::default();
        s.line(&ev(json!({"type": "error", "message": "Reconnecting... 1/5"})));
        s.line(&ev(json!({"type": "item.completed", "item": {"id": "m", "type": "agent_message", "text": "Mostly coding."}})));
        s.line(&ev(json!({"type": "turn.completed", "usage": {"input_tokens": 1, "cached_input_tokens": 0, "output_tokens": 1}})));
        assert_eq!(s.finish(Some(0), "", "").unwrap().text, "Mostly coding.");
        // An old Codex without --ignore-user-config: classified as outdated.
        let e = Stream::default().finish(Some(2), "", "error: unexpected argument '--ignore-user-config' found\n\nUsage: codex exec [OPTIONS] [PROMPT]").unwrap_err();
        let m = e.downcast_ref::<CliFailed>().unwrap().message.clone();
        assert!(m.starts_with("This chat needs a newer version of codex"), "{m}");
        assert_eq!(crate::llm::failure::classify(&m).kind, crate::llm::failure::FailureKind::CliOutdated);
        // No events, an ERROR line on stderr.
        let e = Stream::default().finish(Some(1), "", "ERROR: required MCP servers failed to initialize: daytrace: program not found").unwrap_err();
        assert!(e.downcast_ref::<CliFailed>().unwrap().message.contains("required MCP servers failed"));
        // Completed without a message.
        let mut s = Stream::default();
        s.line(&ev(json!({"type": "turn.completed", "usage": {}})));
        assert_eq!(s.finish(Some(0), "", "").unwrap_err().to_string(), "The model returned an empty answer.");
    }

    #[test]
    fn prompt_puts_the_instructions_first() {
        let p = prompt(" SYSTEM ", &[("user".into(), "q1".into()), ("assistant".into(), "a1".into())], "q2");
        assert_eq!(p, "SYSTEM\n\n---\n\nEarlier in this conversation:\n\nUser: q1\n\nAssistant: a1\n\nNew question from the user:\n\nq2");
        assert_eq!(prompt("S", &[], "q"), "S\n\n---\n\nq");
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

    fn chat_parts(dir: &std::path::Path, runner: Arc<FakeRunner>) -> (CliBackend, PathBuf) {
        let codex = dir.join("codex");
        let mcp = dir.join("daytrace-mcp");
        std::fs::write(&codex, b"").unwrap();
        std::fs::write(&mcp, b"").unwrap();
        (CliBackend::with_parts(CliTool::Codex, &codex.to_string_lossy(), "gpt-6-luna", runner, Arc::new(NoEnv), dir.join("work"), false), mcp)
    }

    #[tokio::test]
    async fn runs_codex_with_the_chat_flags() {
        let dir = tempfile::tempdir().unwrap();
        let runner = Arc::new(FakeRunner::new(vec![FakeRunner::ok(&transcript(7))]));
        let (backend, mcp) = chat_parts(dir.path(), runner.clone());
        let chat = CodexChat { backend: &backend, mcp_exe: mcp.clone(), data_dir: dir.path().join("data"), timeout: Duration::from_secs(30) };
        let cancel = AtomicBool::new(false);
        let mut steps = Vec::new();
        let out = run(&chat, "SYSTEM", &[("user".into(), "q1".into())], "q2", &cancel, &mut |p| steps.push(p)).await.unwrap();
        assert_eq!(out.text, "You wrote [the parser](card:7).");
        assert_eq!(steps.len(), 7, "thinking first, then the stream's steps");
        let spec = &runner.calls()[0];
        assert_eq!(spec.args[..7], ["exec", "--json", "--ephemeral", "--ignore-user-config", "--skip-git-repo-check", "--sandbox", "read-only"]);
        assert!(spec.args.windows(2).any(|w| w[0] == "-c" && w[1] == "features.shell_tool=false"));
        assert!(spec.args.windows(2).any(|w| w[0] == "-c" && w[1] == format!("mcp_servers.daytrace.command='{}'", mcp.display())));
        assert!(spec.args.iter().any(|a| a.starts_with("mcp_servers.daytrace.enabled_tools=['get_time_breakdown','get_timeline',")));
        assert_eq!(spec.args.last().unwrap(), "-");
        let stdin = String::from_utf8(spec.stdin.clone()).unwrap();
        assert!(stdin.starts_with("SYSTEM\n\n---\n\nEarlier in this conversation:"), "{stdin}");
        assert!(stdin.ends_with("New question from the user:\n\nq2"));

        // Without daytrace-mcp there is nothing to run.
        let chat = CodexChat { backend: &backend, mcp_exe: dir.path().join("missing"), data_dir: dir.path().into(), timeout: Duration::from_secs(5) };
        assert!(run(&chat, "S", &[], "q", &cancel, &mut |_| {}).await.unwrap_err().to_string().contains("isn't next to Daytrace"));
    }

    #[tokio::test]
    async fn a_command_stops_the_run_and_the_mcp_failure_is_explained() {
        let dir = tempfile::tempdir().unwrap();
        let shell = ev(json!({"type": "item.started", "item": {"id": "c", "type": "command_execution", "command": "powershell -c dir", "aggregated_output": "", "exit_code": null, "status": "in_progress"}}));
        let runner = Arc::new(FakeRunner::new(vec![
            FakeRunner::ok(&shell),
            FakeRunner::exit(1, "", "ERROR: required MCP servers failed to initialize: daytrace: The system cannot find the file specified."),
        ]));
        let (backend, mcp) = chat_parts(dir.path(), runner);
        let chat = CodexChat { backend: &backend, mcp_exe: mcp, data_dir: dir.path().into(), timeout: Duration::from_secs(5) };
        let cancel = AtomicBool::new(false);
        let e = run(&chat, "S", &[], "q", &cancel, &mut |_| {}).await.unwrap_err().to_string();
        assert!(e.starts_with("The Codex CLI tried to run a command"), "{e}");
        let e = run(&chat, "S", &[], "q", &cancel, &mut |_| {}).await.unwrap_err().to_string();
        assert!(e.starts_with("The Codex CLI couldn't start Daytrace's MCP server"), "{e}");
    }
}
