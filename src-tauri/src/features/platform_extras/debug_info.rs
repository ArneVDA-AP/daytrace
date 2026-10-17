//! "Copy debug info" (Dayflow: Utilities/DebugLogFormatter.swift, "Copy debug logs"): the
//! settings that matter for a problem, the recent analysis batches with their status and reason,
//! the recent model calls (operation, model, latency, tokens, error) and the end of daytrace.log,
//! as plain text for an issue or a Claude session.
//!
//! Left out: API keys (the stored keys and anything shaped like a key or an Authorization
//! value), the user folder and user name in paths, and every window title and card title of the
//! last 7 days that appears in the text. Screenshots, observations, card summaries and the
//! logged requests and responses are never included.

use crate::config::Settings;
use crate::state::AppState;
use crate::timeutil::local;
use anyhow::Result;
use regex::Regex;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::OnceLock;

pub const BATCHES: usize = 20;
pub const CALLS: usize = 30;
pub const LOG_LINES: usize = 200;
/// Only the end of the log file is read.
const LOG_TAIL_BYTES: u64 = 256 * 1024;
/// Titles seen this recently are taken out of the text.
const TITLE_LOOKBACK_SECS: i64 = 7 * 86400;
/// Shorter titles are common words ("Mail", "New tab"); replacing them would garble the log.
const MIN_TITLE_CHARS: usize = 5;
const MAX_FIELD_CHARS: usize = 400;

pub const KEY_REMOVED: &str = "[key removed]";
pub const TITLE_REMOVED: &str = "[title removed]";

/// What to take out of the text.
#[derive(Debug, Clone, Default)]
pub struct Redactions {
    /// Stored API keys (exact text).
    pub secrets: Vec<String>,
    /// The user's home folder (USERPROFILE / HOME).
    pub home: Option<String>,
    /// The Windows user name.
    pub user: Option<String>,
    /// Window titles and card titles.
    pub titles: Vec<String>,
}

impl Redactions {
    /// Sorts the titles longest first (a title that contains another goes first) and drops short
    /// and duplicate ones.
    pub fn prepare(mut self) -> Redactions {
        self.titles.retain(|t| t.trim().chars().count() >= MIN_TITLE_CHARS);
        self.titles.iter_mut().for_each(|t| *t = t.trim().to_string());
        self.titles.sort_by(|a, b| b.len().cmp(&a.len()).then_with(|| a.cmp(b)));
        self.titles.dedup();
        self.secrets.retain(|s| s.trim().len() >= 8);
        self
    }
}

fn key_patterns() -> &'static [(Regex, &'static str)] {
    static P: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    P.get_or_init(|| {
        [
            // Anthropic, OpenAI and OpenRouter keys.
            (r"\bsk-ant-[A-Za-z0-9_\-]{8,}", KEY_REMOVED),
            (r"\bsk-[A-Za-z0-9_\-]{16,}", KEY_REMOVED),
            // Google API keys and GitHub tokens.
            (r"\bAIza[0-9A-Za-z_\-]{30,}", KEY_REMOVED),
            (r"\bgh[pousr]_[A-Za-z0-9]{20,}", KEY_REMOVED),
            (r"\bgithub_pat_[A-Za-z0-9_]{20,}", KEY_REMOVED),
            // An Authorization value with its scheme, then a "Bearer <token>" on its own, then
            // the value of a key or token field or header.
            (r#"((?i-u:authorization)"?\s*[:=]\s*"?)(?:(?i-u:bearer|basic)\s+)?[^\s",;'}\[]{6,}"#, "${1}[key removed]"),
            (r"((?i-u:bearer))\s+[A-Za-z0-9._~+/=\-]{8,}", "$1 [key removed]"),
            (r#"((?i-u:x-api-key|api[_-]?key|access[_-]?token)"?\s*[:=]\s*"?)[^\s",;'}\[]{6,}"#, "${1}[key removed]"),
        ]
        .into_iter()
        .map(|(p, r)| (Regex::new(p).expect("valid pattern"), r))
        .collect()
    })
}

fn path_patterns() -> &'static [(Regex, &'static str)] {
    static P: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    P.get_or_init(|| {
        [
            // C:\Users\<name>, also JSON-escaped (C:\\Users\\<name>) and with forward slashes.
            (r#"([A-Za-z]:[\\/]+(?i-u:users)[\\/]+)[^\\/\s"'<>]+"#, "${1}<user>"),
            (r#"(/(?:home|Users)/)[^/\s"'<>]+"#, "${1}<user>"),
        ]
        .into_iter()
        .map(|(p, r)| (Regex::new(p).expect("valid pattern"), r))
        .collect()
    })
}

/// Replaces every ASCII-case-insensitive occurrence of `needle` in `hay`.
pub fn replace_ignore_ascii_case(hay: &str, needle: &str, with: &str) -> String {
    if needle.is_empty() {
        return hay.to_string();
    }
    let (h, n) = (hay.as_bytes(), needle.as_bytes());
    let mut out = String::with_capacity(hay.len());
    let mut last = 0;
    let mut i = 0;
    while i + n.len() <= h.len() {
        if h[i..i + n.len()].eq_ignore_ascii_case(n) && hay.is_char_boundary(i) && hay.is_char_boundary(i + n.len()) {
            out.push_str(&hay[last..i]);
            out.push_str(with);
            i += n.len();
            last = i;
        } else {
            i += 1;
        }
    }
    out.push_str(&hay[last..]);
    out
}

/// Takes keys, the user folder and name, and titles out of `text`.
pub fn redact(text: &str, r: &Redactions) -> String {
    let mut t = text.to_string();
    for s in &r.secrets {
        t = t.replace(s.trim(), KEY_REMOVED);
    }
    for (re, with) in key_patterns() {
        t = re.replace_all(&t, *with).into_owned();
    }
    if let Some(home) = r.home.as_deref().map(str::trim).filter(|h| h.len() > 3) {
        let forward = home.replace('\\', "/");
        let escaped = home.replace('\\', "\\\\");
        for form in [escaped.as_str(), home, forward.as_str()] {
            t = replace_ignore_ascii_case(&t, form, "%USERPROFILE%");
        }
    }
    for (re, with) in path_patterns() {
        t = re.replace_all(&t, *with).into_owned();
    }
    for title in &r.titles {
        if t.contains(title.as_str()) {
            t = t.replace(title.as_str(), TITLE_REMOVED);
        }
    }
    if let Some(user) = r.user.as_deref().map(str::trim).filter(|u| u.chars().count() >= 3) {
        // Whole words only: a user named "ann" must not change "annotate".
        if let Ok(re) = Regex::new(&format!(r"(^|[^A-Za-z0-9_]){}($|[^A-Za-z0-9_])", regex::escape(user))) {
            // Twice, for neighbours that share a separator ("ann ann").
            for _ in 0..2 {
                t = re.replace_all(&t, "${1}<user>${2}").into_owned();
            }
        }
    }
    t
}

/// The last `n` lines of `text`.
pub fn last_lines(text: &str, n: usize) -> Vec<&str> {
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(n)..].to_vec()
}

/// The end of the log file: the last `LOG_LINES` lines of its last 256 KB.
pub fn read_log_tail(path: &Path) -> std::io::Result<Vec<String>> {
    let mut f = std::fs::File::open(path)?;
    let len = f.metadata()?.len();
    let start = len.saturating_sub(LOG_TAIL_BYTES);
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);
    // The first line may start mid-way when the file was cut.
    let text = if start > 0 { text.split_once('\n').map(|(_, rest)| rest.to_string()).unwrap_or_default() } else { text.into_owned() };
    Ok(last_lines(&text, LOG_LINES).into_iter().map(String::from).collect())
}

fn one_line(s: &str) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    crate::llm::client::truncate(&flat, MAX_FIELD_CHARS)
}

fn clock(ts: i64) -> String {
    local(ts).format("%Y-%m-%d %H:%M:%S").to_string()
}

/// The server of a provider without user name, password, path or query (they can carry keys).
pub fn server_only(url: &str) -> String {
    match reqwest::Url::parse(url.trim()) {
        Ok(u) => match (u.scheme(), u.host_str()) {
            (s, Some(h)) => format!("{s}://{h}{}", u.port().map(|p| format!(":{p}")).unwrap_or_default()),
            _ => String::new(),
        },
        Err(_) if url.trim().is_empty() => String::new(),
        Err(_) => "(a path or an unreadable address)".into(),
    }
}

/// The titles to take out: window titles and card titles of the last 7 days.
fn recent_titles(state: &AppState, now: i64) -> Vec<String> {
    let c = state.db.conn();
    let mut out = Vec::new();
    let since = now - TITLE_LOOKBACK_SECS;
    for sql in [
        "SELECT DISTINCT window_title FROM screenshots WHERE captured_at >= ?1 AND window_title IS NOT NULL LIMIT 20000",
        "SELECT DISTINCT title FROM timeline_cards WHERE start_ts >= ?1 LIMIT 20000",
    ] {
        if let Ok(mut st) = c.prepare(sql)
            && let Ok(rows) = st.query_map([since], |r| r.get::<_, String>(0))
        {
            out.extend(rows.flatten());
        }
    }
    out
}

fn settings_lines(s: &Settings) -> Vec<String> {
    let fallback = match s.fallback_provider {
        Some(k) => format!("{} · {} · {}", k.label(), if s.fallback_model.is_empty() { "(default model)" } else { &s.fallback_model }, server_only(&s.fallback_base_url)),
        None => "off".into(),
    };
    vec![
        format!("Provider: {} · model {} · {}", s.provider.label(), s.model, server_only(&s.base_url)),
        format!("Mode: {:?} · Batch API {} · reasoning {:?} · language {}", s.pipeline, if s.use_batch_api { "on" } else { "off" }, s.reasoning, if s.output_language.is_empty() { "default" } else { &s.output_language }),
        format!("Fallback provider: {fallback}"),
        format!(
            "Capture: every {} s, {} px high, JPEG {} · idle guard {} s · {} private apps · retention {} days · storage limit {}",
            s.capture_interval_secs,
            s.capture_height,
            s.jpeg_quality,
            s.idle_pause_secs,
            s.blocked_apps.len(),
            s.retention_days,
            if s.storage_limit_gb == 0 { "off".to_string() } else { format!("{} GB", s.storage_limit_gb) }
        ),
        format!(
            "Recording: {} · setup {} · launch at sign-in {} · updates: check {}, install automatically {}",
            match super::pause::mode(s) {
                super::pause::Mode::Active => "on".to_string(),
                super::pause::Mode::PausedUntil(t) => format!("paused until {}", clock(t)),
                super::pause::Mode::PausedIndefinite => "paused until resumed".to_string(),
                super::pause::Mode::Setup => "waiting for the first-run setup".to_string(),
            },
            if s.onboarding_completed { "done" } else { "not done" },
            s.launch_at_login,
            s.check_updates,
            s.auto_install_updates
        ),
    ]
}

/// Builds the report. `log_path` is daytrace.log; `r` what to take out (titles are added here).
pub fn build(state: &AppState, log_path: &Path, now: i64, mut r: Redactions, extra: &[String]) -> Result<String> {
    r.titles.extend(recent_titles(state, now));
    let r = r.prepare();
    let s = state.settings();
    let mut out: Vec<String> = vec![
        "# Daytrace debug info".into(),
        format!("Daytrace {} · {} {} · made {} ({})", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH, clock(now), crate::timeutil::local(now).format("%:z")),
        String::new(),
        "## Settings".into(),
    ];
    out.extend(settings_lines(&s));
    out.push(String::new());
    out.push("## State".into());
    let recorder = state.recorder.lock().unwrap().clone();
    let analysis = state.analysis.lock().unwrap().clone();
    out.push(format!("Recorder: {recorder:?}"));
    out.push(format!(
        "Analysis: batch {} · step {} · last success {} · last error {}",
        analysis.current_batch.map(|b| b.to_string()).unwrap_or_else(|| "none".into()),
        analysis.current_step.unwrap_or_else(|| "none".into()),
        analysis.last_success_at.map(clock).unwrap_or_else(|| "none".into()),
        analysis.last_error.as_deref().map(one_line).unwrap_or_else(|| "none".into())
    ));
    out.extend(extra.iter().cloned());

    let c = state.db.conn();
    out.push(String::new());
    out.push(format!("## Last {BATCHES} batches"));
    out.push("id | start – end | status | reason | tries".into());
    let mut st = c.prepare("SELECT id, batch_start_ts, batch_end_ts, status, reason, attempts FROM analysis_batches ORDER BY batch_start_ts DESC, id DESC LIMIT ?")?;
    let rows = st.query_map([BATCHES as i64], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?, r.get::<_, String>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, i64>(5)?))
    })?;
    let mut any = false;
    for (id, a, b, status, reason, tries) in rows.flatten() {
        any = true;
        out.push(format!("{id} | {} – {} | {status} | {} | {tries}", clock(a), local(b).format("%H:%M:%S"), reason.as_deref().map(one_line).unwrap_or_default()));
    }
    if !any {
        out.push("(none)".into());
    }
    drop(st);

    out.push(String::new());
    out.push(format!("## Last {CALLS} model calls"));
    out.push("time (UTC) | batch | operation | model | latency | tokens in/out | error".into());
    let mut st = c.prepare("SELECT created_at, batch_id, operation, model, latency_ms, input_tokens, output_tokens, error FROM llm_calls ORDER BY id DESC LIMIT ?")?;
    let rows = st.query_map([CALLS as i64], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, Option<i64>>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<i64>>(4)?,
            r.get::<_, Option<i64>>(5)?,
            r.get::<_, Option<i64>>(6)?,
            r.get::<_, Option<String>>(7)?,
        ))
    })?;
    let mut any = false;
    let n = |v: Option<i64>| v.map(|x| x.to_string()).unwrap_or_else(|| "?".into());
    for (at, batch, op, model, latency, tin, tout, error) in rows.flatten() {
        any = true;
        out.push(format!(
            "{at} | {} | {op} | {} | {} ms | {}/{} | {}",
            batch.map(|b| b.to_string()).unwrap_or_else(|| "-".into()),
            model.unwrap_or_default(),
            n(latency),
            n(tin),
            n(tout),
            error.as_deref().map(one_line).unwrap_or_else(|| "ok".into())
        ));
    }
    if !any {
        out.push("(none)".into());
    }
    drop(st);
    drop(c);

    out.push(String::new());
    out.push(format!("## daytrace.log (last {LOG_LINES} lines)"));
    match read_log_tail(log_path) {
        Ok(lines) if !lines.is_empty() => out.extend(lines),
        Ok(_) => out.push("(empty)".into()),
        Err(e) => out.push(format!("(could not read the log: {e})")),
    }
    Ok(redact(&out.join("\n"), &r) + "\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Paths, ProviderKind};

    fn r() -> Redactions {
        Redactions {
            secrets: vec!["my-custom-secret-123".into()],
            home: Some(r"C:\Users\Jane Doe".into()),
            user: Some("Jane Doe".into()),
            titles: vec!["Quarterly plan.docx - Word".into(), "Mail".into(), "Quarterly plan".into()],
        }
        .prepare()
    }

    #[test]
    fn takes_keys_out() {
        let t = redact(
            r#"key sk-ant-api03-AbC_dEf-123456 and sk-or-v1-0123456789abcdef0123 and AIzaSyA0123456789abcdefghijklmnopqrstu; Authorization: Bearer abc.def.ghi-123; sent bearer tok.en-456789; {"x-api-key": "zzzzzzzzzz"}; api_key=qqqqqqqq; also my-custom-secret-123"#,
            &r(),
        );
        assert!(!t.contains("AbC_dEf") && !t.contains("0123456789abcdef0123") && !t.contains("AIzaSy"), "{t}");
        assert!(!t.contains("abc.def.ghi") && !t.contains("tok.en") && !t.contains("zzzzzzzzzz") && !t.contains("qqqqqqqq") && !t.contains("my-custom-secret"), "{t}");
        assert!(t.contains("Authorization: [key removed];") && t.contains("bearer [key removed]"), "{t}");
        assert!(t.contains(r#""x-api-key": "[key removed]""#), "{t}");
        // Model names and ordinary words stay.
        let plain = redact("model claude-haiku-5-5 task-runner-for-everything-here sk-short", &r());
        assert_eq!(plain, "model claude-haiku-5-5 task-runner-for-everything-here sk-short");
    }

    #[test]
    fn takes_the_user_folder_and_name_out() {
        let t = redact(
            r#"C:\Users\Jane Doe\AppData\Roaming\dev.daytrace.app; c:/users/jane doe/x; "C:\\Users\\Jane Doe\\y"; D:\Users\other\z; /home/jane/q; signed in as Jane Doe."#,
            &r(),
        );
        assert!(!t.contains("Jane") && !t.contains("jane") && !t.contains("other"), "{t}");
        assert!(t.contains(r"%USERPROFILE%\AppData\Roaming\dev.daytrace.app"), "{t}");
        assert!(t.contains(r"D:\Users\<user>\z") && t.contains("/home/<user>/q") && t.contains("signed in as <user>."), "{t}");
        // Whole words only.
        let r2 = Redactions { user: Some("ann".into()), ..Default::default() };
        assert_eq!(redact("ann annotated ann, joann", &r2), "<user> annotated <user>, joann");
    }

    #[test]
    fn takes_titles_out_longest_first() {
        let t = redact("clamping card 'Quarterly plan.docx - Word' then 'Quarterly plan' in Mail", &r());
        assert_eq!(t, format!("clamping card '{TITLE_REMOVED}' then '{TITLE_REMOVED}' in Mail"), "short titles stay");
    }

    #[test]
    fn reads_the_end_of_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("daytrace.log");
        let body: String = (0..1000).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&p, &body).unwrap();
        let lines = read_log_tail(&p).unwrap();
        assert_eq!(lines.len(), LOG_LINES);
        assert_eq!(lines.last().unwrap(), "line 999");
        assert_eq!(lines[0], format!("line {}", 1000 - LOG_LINES));
        assert_eq!(server_only("https://user:pw@openrouter.ai/api/v1?key=x"), "https://openrouter.ai");
        assert_eq!(server_only("http://localhost:11434"), "http://localhost:11434");
        assert_eq!(server_only(""), "");
    }

    #[test]
    fn report_has_batches_calls_and_log_without_private_text() {
        let dir = tempfile::tempdir().unwrap();
        let st = AppState::new(Paths::new(dir.path().to_path_buf()).unwrap()).unwrap();
        crate::features::platform_extras::update_settings(&st, |s| {
            s.provider = ProviderKind::OpenaiCompatible;
            s.base_url = "https://user:secretpw@example.com/v1".into();
            s.onboarding_completed = true;
        })
        .unwrap();
        let now = 1_800_000_000;
        st.db.insert_screenshot(now - 900, "", 0, 0, Some("winword"), Some("Salary review 2026.docx - Word"), None, None).unwrap();
        let b = st.db.create_batch(&st.db.unbatched_screenshots(0).unwrap()).unwrap();
        st.db.set_batch_status(b, "failed", Some("HTTP 401: invalid x-api-key: sk-ant-api03-ZZZZZZZZZZZZ")).unwrap();
        st.db.log_llm_call(Some(b), "transcribe", "m1", 1234, "{\"secret request\":1}", None, Some("Invalid JSON: Salary review 2026.docx - Word"), (Some(10), Some(2)));
        let log = dir.path().join("daytrace.log");
        std::fs::write(&log, "[INFO] data dir /home/someone/.local/share/dev.daytrace.app\n[INFO] clamping card 'Salary review 2026.docx - Word'\n").unwrap();
        let text = build(&st, &log, now, Redactions::default(), &["Update: none".into()]).unwrap();
        assert!(text.contains(&format!("{b} | ")) && text.contains("| failed |"), "{text}");
        assert!(text.contains("| transcribe | m1 | 1234 ms | 10/2 |"), "{text}");
        assert!(text.contains("Provider: OpenAI-compatible API · model ") && text.contains("https://example.com"), "{text}");
        assert!(text.contains("Update: none"));
        assert!(!text.contains("Salary review"), "window title taken out: {text}");
        assert!(!text.contains("ZZZZZZZZ") && !text.contains("secretpw") && !text.contains("secret request"), "{text}");
        assert!(!text.contains("someone") && text.contains("/home/<user>/"), "{text}");
    }
}
