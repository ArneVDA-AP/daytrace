//! Reading what the CLIs print (Dayflow ChatCLIProcessRunner.parseAssistant /
//! parseClaudeJSONEnvelope, AgentCLISupport+Parsing.extractCLIError).

use serde_json::Value;

/// Token counts reported by `claude -p --output-format json`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Usage {
    pub input: i64,
    pub cached_input: i64,
    pub cache_creation_input: i64,
    pub output: i64,
}

impl Usage {
    /// (input, output) for the call log: every input token, cached or not.
    pub fn for_log(&self) -> (Option<i64>, Option<i64>) {
        (Some(self.input + self.cached_input + self.cache_creation_input), Some(self.output))
    }
    pub fn add(self, other: Option<Usage>) -> Usage {
        let Some(o) = other else { return self };
        Usage {
            input: self.input + o.input,
            cached_input: self.cached_input + o.cached_input,
            cache_creation_input: self.cache_creation_input + o.cache_creation_input,
            output: self.output + o.output,
        }
    }
}

/// The `result` event of a Claude CLI run.
#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeEnvelope {
    pub text: String,
    pub usage: Option<Usage>,
    pub session_id: Option<String>,
    /// Claude Code sets this when the run failed (signed out, usage limit, API error); `text`
    /// then holds the reason.
    pub is_error: bool,
}

/// Removes ANSI CSI sequences (`ESC [ ... final`), as Dayflow's stripANSIEscapes.
pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for n in chars.by_ref() {
                if ('\u{40}'..='\u{7e}').contains(&n) {
                    break;
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn envelope_from_value(v: &Value) -> Option<ClaudeEnvelope> {
    let events: Vec<&Value> = match v {
        Value::Array(a) => a.iter().collect(),
        Value::Object(_) => vec![v],
        _ => return None,
    };
    let result = events.iter().rev().find(|e| e["type"] == "result" && e["result"].is_string())?;
    let usage = result.get("usage").filter(|u| u.is_object()).map(|u| Usage {
        input: u["input_tokens"].as_i64().unwrap_or(0),
        cached_input: u["cache_read_input_tokens"].as_i64().unwrap_or(0),
        cache_creation_input: u["cache_creation_input_tokens"].as_i64().unwrap_or(0),
        output: u["output_tokens"].as_i64().unwrap_or(0),
    });
    let session_id = result["session_id"]
        .as_str()
        .map(String::from)
        .or_else(|| events.iter().rev().find_map(|e| e["session_id"].as_str().map(String::from)));
    Some(ClaudeEnvelope {
        text: result["result"].as_str().unwrap_or_default().to_string(),
        usage,
        session_id,
        is_error: result["is_error"].as_bool().unwrap_or(false),
    })
}

/// Top-level `{...}` / `[...]` values inside other text (login-shell noise, warnings).
fn balanced_json_values(s: &str) -> Vec<&str> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    while start < bytes.len() {
        if bytes[start] != b'{' && bytes[start] != b'[' {
            start += 1;
            continue;
        }
        match balanced_end(bytes, start) {
            Some(end) => {
                out.push(&s[start..=end]);
                start = end + 1;
            }
            None => start += 1,
        }
    }
    out
}

fn balanced_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut stack: Vec<u8> = Vec::new();
    let (mut in_str, mut esc) = (false, false);
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            if esc {
                esc = false;
            } else if b == b'\\' {
                esc = true;
            } else if b == b'"' {
                in_str = false;
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => stack.push(b'}'),
            b'[' => stack.push(b']'),
            b'}' | b']' => {
                if stack.last() != Some(&b) {
                    return None;
                }
                stack.pop();
                if stack.is_empty() {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// The result envelope of `claude -p --output-format json` (a single object, or the event array
/// that `--verbose` prints). Falls back to scanning for exactly one result inside other output.
pub fn parse_claude_output(raw: &str) -> Option<ClaudeEnvelope> {
    let cleaned = strip_ansi(raw);
    let trimmed = cleaned.trim();
    if let Ok(v) = serde_json::from_str::<Value>(trimmed) {
        if let Some(e) = envelope_from_value(&v) {
            return Some(e);
        }
    }
    let found: Vec<ClaudeEnvelope> = balanced_json_values(trimmed)
        .into_iter()
        .filter_map(|c| serde_json::from_str::<Value>(c).ok())
        .filter_map(|v| envelope_from_value(&v))
        .collect();
    if found.len() == 1 { found.into_iter().next() } else { None }
}

/// Codex's answer: the `--output-last-message` file when it was written, else the text after the
/// last `codex` marker line of stdout (older versions), else all of stdout.
pub fn parse_codex_output(last_message: Option<&str>, stdout: &str) -> String {
    if let Some(m) = last_message.map(str::trim).filter(|m| !m.is_empty()) {
        return m.to_string();
    }
    let trimmed = stdout.trim();
    if let Some(i) = trimmed.rfind("\ncodex\n") {
        let mut resp = &trimmed[i + "\ncodex\n".len()..];
        if let Some(j) = resp.to_ascii_lowercase().find("\ntokens used") {
            resp = &resp[..j];
        }
        return resp.trim().to_string();
    }
    trimmed.to_string()
}

/// The CLI's own error line, if it printed one (Dayflow extractCLIError): `ERROR:` lines on
/// stderr (Codex), `API Error:` / `Invalid API key` / `You've hit your limit` on stdout (Claude).
pub fn extract_cli_error(stdout: &str, stderr: &str) -> Option<String> {
    for line in stderr.lines().map(str::trim) {
        if line.starts_with("ERROR:") {
            return Some(line.to_string());
        }
    }
    static TRAILING_OSC: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = TRAILING_OSC.get_or_init(|| regex::Regex::new(r"\][\d;]+$").unwrap());
    for line in stdout.lines().map(str::trim) {
        if line.starts_with("API Error:") || line.starts_with("Invalid API key") || line.starts_with("You've hit your limit") {
            return Some(re.replace(line, "").to_string());
        }
    }
    None
}

/// Dayflow ChatCLIRunResult.failureMessage: stderr, else the (parsed) stdout, else the exit code.
pub fn failure_message(tool_name: &str, code: Option<i32>, stdout: &str, stderr: &str) -> String {
    let err = stderr.trim();
    if !err.is_empty() {
        return crate::llm::client::truncate(err, 1000);
    }
    let out = stdout.trim();
    if !out.is_empty() {
        return crate::llm::client::truncate(out, 1000);
    }
    match code {
        Some(c) => format!("{tool_name} exited with code {c}"),
        None => format!("{tool_name} was stopped"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Shapes of `claude -p --output-format json` output, as Dayflow's ClaudeNonStreamingEvent
    // decodes them.
    const SINGLE: &str = r#"{"type":"result","subtype":"success","is_error":false,"duration_ms":41200,"num_turns":16,"result":"[{\"startTime\":\"9:00 AM\"}]","session_id":"6a1f0c1e-1111-4a4a-8b8b-000000000001","total_cost_usd":0.21,"usage":{"input_tokens":12,"cache_creation_input_tokens":2048,"cache_read_input_tokens":30000,"output_tokens":1500}}"#;

    #[test]
    fn parses_single_result_object() {
        let e = parse_claude_output(SINGLE).unwrap();
        assert_eq!(e.text, r#"[{"startTime":"9:00 AM"}]"#);
        assert!(!e.is_error);
        assert_eq!(e.session_id.as_deref(), Some("6a1f0c1e-1111-4a4a-8b8b-000000000001"));
        let u = e.usage.unwrap();
        assert_eq!(u.for_log(), (Some(32060), Some(1500)));
    }

    #[test]
    fn parses_verbose_event_array() {
        let raw = r#"[{"type":"system","subtype":"init","session_id":"s-1","tools":["Read"]},
            {"type":"assistant","message":{"content":[{"type":"text","text":"x"}]},"session_id":"s-1"},
            {"type":"result","subtype":"success","is_error":false,"result":"hello","usage":{"input_tokens":5,"output_tokens":2}}]"#;
        let e = parse_claude_output(raw).unwrap();
        assert_eq!(e.text, "hello");
        assert_eq!(e.session_id.as_deref(), Some("s-1"));
        assert_eq!(e.usage.unwrap().for_log(), (Some(5), Some(2)));
    }

    #[test]
    fn finds_result_inside_noise_and_reads_errors() {
        let raw = format!("\u{1b}[33mwarning: something\u{1b}[0m\n{}\n", r#"{"type":"result","subtype":"success","is_error":true,"result":"Invalid API key · Please run /login"}"#);
        let e = parse_claude_output(&raw).unwrap();
        assert!(e.is_error);
        assert_eq!(e.text, "Invalid API key · Please run /login");
        assert!(parse_claude_output("plain text answer").is_none());
        // Two different results: ambiguous, as in Dayflow.
        let two = format!("{SINGLE}\n{SINGLE}");
        assert!(parse_claude_output(&two).is_none());
    }

    #[test]
    fn codex_answer_sources() {
        assert_eq!(parse_codex_output(Some(" {\"cards\":[]} \n"), "ignored"), "{\"cards\":[]}");
        let legacy = "[2026-01-01] OpenAI Codex\nthinking\nhmm\ncodex\n{\"cards\":[]}\ntokens used\n1,234";
        assert_eq!(parse_codex_output(None, legacy), "{\"cards\":[]}");
        assert_eq!(parse_codex_output(Some(""), "  plain  "), "plain");
    }

    #[test]
    fn cli_error_lines() {
        assert_eq!(
            extract_cli_error("", "starting\nERROR: You've hit your usage limit. Try again later.\n").as_deref(),
            Some("ERROR: You've hit your usage limit. Try again later.")
        );
        assert_eq!(
            extract_cli_error("You've hit your limit · resets 3pm (Europe/Brussels)]9;4;0;\n", "").as_deref(),
            Some("You've hit your limit · resets 3pm (Europe/Brussels)")
        );
        assert_eq!(extract_cli_error("all good", "fine"), None);
        assert_eq!(failure_message("claude", Some(1), "", ""), "claude exited with code 1");
        assert_eq!(failure_message("claude", Some(1), "Not logged in", ""), "Not logged in");
    }
}
