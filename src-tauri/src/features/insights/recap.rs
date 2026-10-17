//! Daily recap (standup), ported from Dayflow (MIT, (c) 2025 Jerry Liu):
//! Core/AI/DailyRecapGenerator.swift (cards text, prompt assembly, response parsing),
//! Core/AI/DailyRecapModels.swift (source day resolution), Core/AI/DailyRecapScheduler.swift and
//! DailyRecapAttemptBudget.swift (automatic generation), Views/UI/DailyView+Standup.swift and
//! DailyView+Workflow.swift (regenerate, minimum activity).
//!
//! A recap is stored per target day (the day the standup is for, Dayflow's standup_day). Its
//! highlights come from a source day: the most recent of the three days before with enough
//! captured activity that no other recap has used yet. The local-provider path of Dayflow returns
//! `done` (highlights) and `next` (one task); blockers stay empty until the user types them.

use super::prompts::{EVIDENCE_PROMPT, LOCAL_PROMPT, SOL_EVIDENCE_RULES};
use super::stats::{cat_info, Kind};
use crate::config::ProviderKind;
use crate::db::StoredCard;
use crate::llm::prompts::language_instruction;
use crate::timeutil;
use crate::types::Category;
use anyhow::{anyhow, Result};
use chrono::{Duration, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashSet;

pub const LOOKBACK_DAYS: i64 = 3;
/// Minimum captured minutes on a source day: the Daily view uses 120, the scheduler 180.
pub const MIN_MINUTES_MANUAL: i64 = 120;
pub const MIN_MINUTES_AUTO: i64 = 180;
pub const MAX_DONE: usize = 5;
pub const MAX_OUTPUT_TOKENS: u32 = 8192;
/// Prompts above this many characters drop card summaries (about 55k input tokens).
const MAX_PROMPT_CHARS: usize = 190_000;

pub const INSUFFICIENT_HISTORY: &str = "Not enough captured activity in the previous 3 days to generate a standup.";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecapEntry {
    pub target_day: String,
    pub source_day: String,
    pub highlights: Vec<String>,
    pub tasks: Vec<String>,
    pub blockers: Vec<String>,
    pub provider: String,
    pub model: String,
    /// The user changed the generated text.
    pub edited: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

// ---- storage ----

const COLS: &str = "target_day, source_day, highlights, tasks, blockers, provider, model, edited, created_at, updated_at";

fn list_from(s: Option<String>) -> Vec<String> {
    s.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

fn entry_from_row(r: &rusqlite::Row) -> rusqlite::Result<RecapEntry> {
    Ok(RecapEntry {
        target_day: r.get(0)?,
        source_day: r.get(1)?,
        highlights: list_from(r.get(2)?),
        tasks: list_from(r.get(3)?),
        blockers: list_from(r.get(4)?),
        provider: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
        model: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
        edited: r.get::<_, i64>(7)? != 0,
        created_at: r.get(8)?,
        updated_at: r.get(9)?,
    })
}

pub fn load(conn: &Connection, target_day: &str) -> Result<Option<RecapEntry>> {
    Ok(conn
        .query_row(&format!("SELECT {COLS} FROM daily_recaps WHERE target_day = ?"), [target_day], entry_from_row)
        .optional()?)
}

/// The most recently updated recap made from `source_day`.
pub fn load_by_source(conn: &Connection, source_day: &str) -> Result<Option<RecapEntry>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLS} FROM daily_recaps WHERE source_day = ? ORDER BY updated_at DESC LIMIT 1"),
            [source_day],
            entry_from_row,
        )
        .optional()?)
}

pub fn save(conn: &Connection, e: &RecapEntry) -> Result<()> {
    let j = |v: &Vec<String>| serde_json::to_string(v).unwrap_or_else(|_| "[]".into());
    conn.execute(
        "INSERT INTO daily_recaps (target_day, source_day, highlights, tasks, blockers, provider, model, edited, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
         ON CONFLICT(target_day) DO UPDATE SET source_day = excluded.source_day, highlights = excluded.highlights,
           tasks = excluded.tasks, blockers = excluded.blockers, provider = excluded.provider, model = excluded.model,
           edited = excluded.edited, updated_at = excluded.updated_at",
        params![e.target_day, e.source_day, j(&e.highlights), j(&e.tasks), j(&e.blockers), e.provider, e.model, e.edited as i64, e.created_at, e.updated_at],
    )?;
    Ok(())
}

/// Source days already used by recaps of other target days (DailyRecapSourceDayResolver.consumedSourceDays).
pub fn consumed_source_days(conn: &Connection, excluding_target: &str) -> Result<HashSet<String>> {
    let mut st = conn.prepare("SELECT source_day FROM daily_recaps WHERE target_day != ?")?;
    let rows = st.query_map([excluding_target], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<HashSet<_>>>()?;
    Ok(rows)
}

/// Captured minutes of a logical day: cards starting in it, without System and Idle time
/// (Dayflow's hasMinimumTimelineActivity counts every card; idle time says nothing to recap).
pub fn activity_minutes(conn: &Connection, day: &str, cats: &[Category]) -> Result<i64> {
    let Some((from, to)) = timeutil::logical_day_bounds(day) else { return Ok(0) };
    let mut st = conn.prepare("SELECT category, end_ts - start_ts FROM timeline_cards WHERE is_deleted = 0 AND start_ts >= ? AND start_ts < ?")?;
    let rows = st.query_map([from, to], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    let mut secs = 0;
    for row in rows {
        let (cat, d) = row?;
        if matches!(cat_info(&cat, cats).kind, Kind::Focus | Kind::Distraction) {
            secs += d.max(0);
        }
    }
    Ok(secs / 60)
}

/// DailyRecapSourceDayResolver.sourceDay: the most recent of the `lookback` days before `target`
/// that is not consumed and has enough activity.
pub fn resolve_source_day(target: &str, lookback: i64, consumed: &HashSet<String>, mut has_min_activity: impl FnMut(&str) -> bool) -> Option<String> {
    let date = NaiveDate::parse_from_str(target, "%Y-%m-%d").ok()?;
    (1..=lookback)
        .map(|o| (date - Duration::days(o)).format("%Y-%m-%d").to_string())
        .find(|d| !consumed.contains(d) && has_min_activity(d))
}

/// Source day for a target day as the Daily view resolves it.
pub fn source_for(conn: &Connection, target: &str, cats: &[Category], min_minutes: i64) -> Result<Option<String>> {
    let consumed = consumed_source_days(conn, target)?;
    let mut err = None;
    let found = resolve_source_day(target, LOOKBACK_DAYS, &consumed, |d| match activity_minutes(conn, d, cats) {
        Ok(m) => m >= min_minutes,
        Err(e) => {
            err = Some(e);
            false
        }
    });
    match (found, err) {
        (None, Some(e)) => Err(e),
        (f, _) => Ok(f),
    }
}

// ---- prompt ----

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptStyle {
    /// Dayflow's local and Gemini path.
    Local,
    /// Dayflow's Claude path.
    Evidence,
    /// Dayflow's ChatGPT path: the evidence prompt plus extra checks.
    EvidenceExtra,
}

pub fn prompt_style(kind: ProviderKind, base_url: &str) -> PromptStyle {
    match kind {
        ProviderKind::Anthropic | ProviderKind::ClaudeCli => PromptStyle::Evidence,
        ProviderKind::CodexCli => PromptStyle::EvidenceExtra,
        ProviderKind::Ollama | ProviderKind::LmStudio => PromptStyle::Local,
        ProviderKind::OpenaiCompatible => {
            if base_url.to_lowercase().contains("generativelanguage.googleapis.com") {
                PromptStyle::Local
            } else {
                PromptStyle::EvidenceExtra
            }
        }
    }
}

fn clock_lower(ts: i64) -> String {
    timeutil::local(ts).format("%-I:%M%P").to_string()
}

/// DailyRecapGenerator.makeCardsText. System (error) cards are left out.
pub fn cards_text(day: &str, cards: &[StoredCard], with_summaries: bool) -> String {
    let mut ordered: Vec<&StoredCard> = cards.iter().filter(|c| !c.category.trim().eq_ignore_ascii_case("system")).collect();
    ordered.sort_by(|a, b| a.start_ts.cmp(&b.start_ts).then(a.end_ts.cmp(&b.end_ts)));
    if ordered.is_empty() {
        return format!("No timeline activities were recorded for {day}.");
    }
    let mut lines = vec![format!("Timeline activities for {day}:"), String::new()];
    for (i, c) in ordered.iter().enumerate() {
        let title = c.title.trim();
        let summary = c.summary.trim();
        let line = if !title.is_empty() {
            title
        } else if !summary.is_empty() {
            summary
        } else {
            "Untitled activity"
        };
        lines.push(format!("{}. {} - {}: {}", i + 1, clock_lower(c.start_ts), clock_lower(c.end_ts), line));
        if with_summaries && !summary.is_empty() && summary != line {
            lines.push(format!("   {summary}"));
        }
    }
    lines.join("\n")
}

/// DailyRecapGenerator.makeLocalPrompt.
pub fn build_prompt(day: &str, cards: &[StoredCard], style: PromptStyle, language: &str) -> String {
    let instructions = match style {
        PromptStyle::Local => LOCAL_PROMPT.to_string(),
        PromptStyle::Evidence => EVIDENCE_PROMPT.to_string(),
        PromptStyle::EvidenceExtra => format!("{EVIDENCE_PROMPT}\n\nAdditional evidence checks:\n{SOL_EVIDENCE_RULES}"),
    };
    let language = language_instruction(language, true).map(|l| format!("## Language\n\n{l}")).unwrap_or_default();
    let assemble = |cards_text: String| {
        format!(
            "{instructions}\n\nYou only have timeline cards for this day. The log is incomplete by nature, so prefer omission over guessing.\n\nActivity log:\n\n{cards_text}\n\n{language}\n\n## Output format\n\nReturn ONLY valid JSON, no markdown fences, no preamble. Use this exact schema:\n\n{{\n  \"done\": [\"first bullet\", \"second bullet\", \"...\"],\n  \"next\": \"one sentence or null\"\n}}\n\nReturn exactly one JSON object and nothing before or after it."
        )
    };
    let full = assemble(cards_text(day, cards, true));
    if full.chars().count() <= MAX_PROMPT_CHARS {
        return full;
    }
    assemble(cards_text(day, cards, false))
}

// ---- response ----

#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub done: Vec<String>,
    pub next: Option<String>,
}

fn shorten(s: &str) -> String {
    crate::llm::client::truncate(s, 600)
}

/// cleanRawModelResponse: drop a thinking preamble and a surrounding code fence.
fn clean(raw: &str) -> String {
    let mut c = raw.trim().to_string();
    const END: &str = "---END_THINKING---";
    if let Some(i) = c.find(END) {
        c = c[i + END.len()..].trim().to_string();
    }
    if c.starts_with("```") {
        if let Some(nl) = c.find('\n') {
            c = c[nl + 1..].to_string();
            if let Some(close) = c.rfind("\n```") {
                c.truncate(close);
            }
        }
    }
    c.trim().to_string()
}

/// extractFirstJSONObject: the first balanced `{…}`, ignoring braces inside strings.
fn first_json_object(text: &str) -> Option<&str> {
    let (mut start, mut depth, mut in_str, mut esc) = (None, 0usize, false, false);
    for (i, ch) in text.char_indices() {
        if esc {
            esc = false;
            continue;
        }
        if ch == '\\' && in_str {
            esc = true;
            continue;
        }
        if ch == '"' {
            in_str = !in_str;
            continue;
        }
        if in_str {
            continue;
        }
        if ch == '{' {
            if depth == 0 {
                start = Some(i);
            }
            depth += 1;
        } else if ch == '}' && depth > 0 {
            depth -= 1;
            if depth == 0 {
                return start.map(|s| &text[s..=i]);
            }
        }
    }
    None
}

fn scalar(v: &Value) -> Option<String> {
    let raw = match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let t = raw.trim();
    (!t.is_empty() && !t.eq_ignore_ascii_case("null")).then(|| t.to_string())
}

fn strings(v: Option<&Value>) -> Vec<String> {
    let items: Vec<&Value> = match v {
        Some(Value::Array(a)) => a.iter().collect(),
        Some(other) => vec![other],
        None => vec![],
    };
    let mut seen = HashSet::new();
    items.into_iter().filter_map(scalar).filter(|s| seen.insert(s.clone())).collect()
}

fn optional_string(v: Option<&Value>) -> Option<String> {
    match v? {
        Value::Null => None,
        Value::Array(a) => a.iter().find_map(scalar),
        other => scalar(other),
    }
}

/// DailyRecapGenerator.parseLocalResponse.
pub fn parse_response(raw: &str) -> Result<Parsed> {
    let cleaned = clean(raw);
    let candidate = first_json_object(&cleaned).unwrap_or(&cleaned);
    let json: Value = serde_json::from_str(candidate).map_err(|_| anyhow!("The model did not return valid JSON.\n\nRAW OUTPUT:\n{}", shorten(raw)))?;
    let Value::Object(obj) = json else {
        return Err(anyhow!("The model did not return valid JSON.\n\nRAW OUTPUT:\n{}", shorten(raw)));
    };
    if !obj.contains_key("done") && !obj.contains_key("next") {
        return Err(anyhow!("The model returned JSON, but it did not match the Daily recap schema.\n\nRAW OUTPUT:\n{}", shorten(raw)));
    }
    Ok(Parsed { done: strings(obj.get("done")).into_iter().take(MAX_DONE).collect(), next: optional_string(obj.get("next")) })
}

/// A line without one leading list marker: `-` or `*` followed by whitespace, or `•`. Text that
/// starts with these characters ("-5% churn", "**Shipped**") is left alone.
fn strip_marker(line: &str) -> &str {
    let t = line.trim();
    if let Some(rest) = t.strip_prefix('•') {
        return rest.trim_start();
    }
    match t.strip_prefix(['-', '*']) {
        Some(rest) if rest.is_empty() || rest.starts_with(char::is_whitespace) => rest.trim_start(),
        _ => t,
    }
}

/// Trimmed, non-empty, de-duplicated lines (the editable lists).
pub fn clean_items(items: &[String]) -> Vec<String> {
    let mut seen = HashSet::new();
    items
        .iter()
        .flat_map(|s| s.lines())
        .map(|s| strip_marker(s).to_string())
        .filter(|s| !s.is_empty() && seen.insert(s.clone()))
        .collect()
}

pub fn provider_label(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "anthropic",
        ProviderKind::Ollama => "ollama",
        ProviderKind::LmStudio => "lm_studio",
        ProviderKind::OpenaiCompatible => "openai_compatible",
        ProviderKind::ClaudeCli => "claude_cli",
        ProviderKind::CodexCli => "codex_cli",
    }
}

/// Builds the entry from a parsed response; errors when nothing usable came back.
pub fn entry_from(parsed: Parsed, target: &str, source: &str, provider: &str, model: &str, now: i64) -> Result<RecapEntry> {
    if parsed.done.is_empty() && parsed.next.is_none() {
        return Err(anyhow!("Daily generation returned no usable items for {source}."));
    }
    Ok(RecapEntry {
        target_day: target.to_string(),
        source_day: source.to_string(),
        highlights: parsed.done,
        tasks: parsed.next.into_iter().collect(),
        blockers: vec![],
        provider: provider.to_string(),
        model: model.to_string(),
        edited: false,
        created_at: now,
        updated_at: now,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::insights::stats::tests::{at, card};
    use crate::types::default_categories;

    fn conn() -> Connection {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(super::super::SCHEMA).unwrap();
        c.execute_batch(
            "CREATE TABLE timeline_cards (id INTEGER PRIMARY KEY, start_ts INTEGER, end_ts INTEGER, category TEXT, is_deleted INTEGER DEFAULT 0);",
        )
        .unwrap();
        c
    }

    #[test]
    fn cards_text_matches_dayflow() {
        let d = (2026, 10, 16);
        let mut a = card(1, at(d, 14, 5), at(d, 14, 30), "Work", "Pricing page FAQ");
        a.summary = "Wrote the FAQ section.".into();
        let mut b = card(2, at(d, 9, 0), at(d, 9, 45), "Work", "");
        b.summary = "Email triage".into();
        let sys = card(3, at(d, 10, 0), at(d, 10, 15), "System", "Processing failed");
        let text = cards_text("2026-10-16", &[a, b, sys], true);
        assert_eq!(
            text,
            "Timeline activities for 2026-10-16:\n\n1. 9:00am - 9:45am: Email triage\n2. 2:05pm - 2:30pm: Pricing page FAQ\n   Wrote the FAQ section."
        );
        assert_eq!(cards_text("2026-10-16", &[], true), "No timeline activities were recorded for 2026-10-16.");
    }

    #[test]
    fn prompt_assembly() {
        let d = (2026, 10, 16);
        let cards = vec![card(1, at(d, 9, 0), at(d, 10, 0), "Work", "Spec review")];
        let p = build_prompt("2026-10-16", &cards, PromptStyle::Evidence, "");
        assert!(p.starts_with("Write a short end-of-day Daily recap for your future self"));
        assert!(p.contains("- Otherwise use null.\n\nYou only have timeline cards for this day."));
        assert!(p.contains("Activity log:\n\nTimeline activities for 2026-10-16:\n\n1. 9:00am - 10:00am: Spec review\n   Spec review summary\n\n\n\n## Output format"));
        assert!(p.ends_with("}\n\nReturn exactly one JSON object and nothing before or after it."));
        assert!(!p.contains("## Language"));

        let p = build_prompt("2026-10-16", &cards, PromptStyle::EvidenceExtra, "Dutch");
        assert!(p.contains("- Otherwise use null.\n\nAdditional evidence checks:\n- Treat visual-access refusals"));
        assert!(p.contains("## Language\n\nThe user only speaks Dutch. Respond in Dutch, but keep JSON keys in English exactly as specified."));

        let p = build_prompt("2026-10-16", &cards, PromptStyle::Local, "English");
        assert!(p.starts_with("# Daily Recap Prompt\n\nYou are the person whose activity log this is"));
        assert!(p.contains("-> What came out of it?\n\n\nYou only have timeline cards"));
    }

    #[test]
    fn long_days_drop_summaries() {
        let d = (2026, 10, 16);
        let mut cards = Vec::new();
        for i in 0..400 {
            let mut c = card(i, at(d, 5, 0) + i * 60, at(d, 5, 1) + i * 60, "Work", &format!("Task {i}"));
            c.summary = "x".repeat(600);
            cards.push(c);
        }
        let p = build_prompt("2026-10-16", &cards, PromptStyle::Evidence, "");
        assert!(p.chars().count() <= MAX_PROMPT_CHARS);
        assert!(p.contains("400. "));
        assert!(!p.contains("xxxx"));
    }

    #[test]
    fn provider_prompt_styles() {
        assert_eq!(prompt_style(ProviderKind::Anthropic, "https://api.anthropic.com"), PromptStyle::Evidence);
        assert_eq!(prompt_style(ProviderKind::Ollama, ""), PromptStyle::Local);
        assert_eq!(prompt_style(ProviderKind::OpenaiCompatible, "https://generativelanguage.googleapis.com/v1beta/openai"), PromptStyle::Local);
        assert_eq!(prompt_style(ProviderKind::OpenaiCompatible, "https://api.openai.com/v1"), PromptStyle::EvidenceExtra);
    }

    #[test]
    fn parses_responses() {
        let p = parse_response(r#"{"done": ["Shipped the pricing page.", " Shipped the pricing page. ", "", "null", 42], "next": null}"#).unwrap();
        assert_eq!(p, Parsed { done: vec!["Shipped the pricing page.".into(), "42".into()], next: None });

        let raw = "thinking...\n---END_THINKING---\n```json\n{\"done\": [\"A {braced} item\"], \"next\": [\"\", \"Send the agreement to Acme.\"]}\n```";
        let p = parse_response(raw).unwrap();
        assert_eq!(p.done, vec!["A {braced} item"]);
        assert_eq!(p.next.as_deref(), Some("Send the agreement to Acme."));

        let p = parse_response("Sure! Here it is: {\"done\": \"Only one\", \"next\": \"  \"} Hope that helps {x}").unwrap();
        assert_eq!(p, Parsed { done: vec!["Only one".into()], next: None });

        let p = parse_response(r#"{"done": ["1","2","3","4","5","6"]}"#).unwrap();
        assert_eq!(p.done.len(), 5);

        let e = parse_response("no json here").unwrap_err().to_string();
        assert!(e.starts_with("The model did not return valid JSON."), "{e}");
        let e = parse_response(r#"{"highlights": []}"#).unwrap_err().to_string();
        assert!(e.starts_with("The model returned JSON, but it did not match the Daily recap schema."), "{e}");
    }

    #[test]
    fn empty_result_is_an_error() {
        let e = entry_from(Parsed { done: vec![], next: None }, "2026-10-17", "2026-10-16", "anthropic", "m", 0).unwrap_err();
        assert_eq!(e.to_string(), "Daily generation returned no usable items for 2026-10-16.");
        let ok = entry_from(Parsed { done: vec![], next: Some("Finish the essay.".into()) }, "2026-10-17", "2026-10-16", "anthropic", "m", 5).unwrap();
        assert_eq!(ok.tasks, vec!["Finish the essay."]);
        assert!(ok.blockers.is_empty());
    }

    #[test]
    fn resolves_source_days() {
        let consumed: HashSet<String> = ["2026-10-16".to_string()].into();
        let active = |d: &str| d != "2026-10-15";
        // 16th is consumed, 15th has no activity → 14th
        assert_eq!(resolve_source_day("2026-10-17", 3, &consumed, active).as_deref(), Some("2026-10-14"));
        assert_eq!(resolve_source_day("2026-10-17", 3, &HashSet::new(), |_| true).as_deref(), Some("2026-10-16"));
        assert_eq!(resolve_source_day("2026-10-17", 3, &HashSet::new(), |_| false), None);
    }

    #[test]
    fn storage_and_activity() {
        let c = conn();
        let cats = default_categories();
        let d = (2026, 10, 16);
        let ins = |s: i64, e: i64, cat: &str| {
            c.execute("INSERT INTO timeline_cards (start_ts, end_ts, category) VALUES (?, ?, ?)", params![s, e, cat]).unwrap();
        };
        ins(at(d, 9, 0), at(d, 11, 0), "Work");
        ins(at(d, 11, 0), at(d, 12, 30), "Idle");
        ins(at(d, 13, 0), at(d, 13, 30), "System");
        assert_eq!(activity_minutes(&c, "2026-10-16", &cats).unwrap(), 120);
        assert_eq!(source_for(&c, "2026-10-17", &cats, 120).unwrap().as_deref(), Some("2026-10-16"));
        assert_eq!(source_for(&c, "2026-10-17", &cats, 180).unwrap(), None);

        let mut e = entry_from(Parsed { done: vec!["Did a thing.".into()], next: None }, "2026-10-17", "2026-10-16", "anthropic", "claude-haiku-5-5", 100).unwrap();
        save(&c, &e).unwrap();
        assert_eq!(load(&c, "2026-10-17").unwrap().unwrap(), e);
        // The 16th is now used by the 17th's recap, so the 18th cannot take it.
        assert_eq!(source_for(&c, "2026-10-18", &cats, 120).unwrap(), None);
        // ...but regenerating the 17th still finds it.
        assert_eq!(source_for(&c, "2026-10-17", &cats, 120).unwrap().as_deref(), Some("2026-10-16"));
        e.blockers = vec!["Waiting on review".into()];
        e.edited = true;
        e.updated_at = 200;
        save(&c, &e).unwrap();
        let back = load_by_source(&c, "2026-10-16").unwrap().unwrap();
        assert_eq!(back.blockers, vec!["Waiting on review"]);
        assert!(back.edited);
        assert_eq!(back.created_at, 100);
    }

    #[test]
    fn cleans_edited_items() {
        let items = vec!["- First\n* Second".to_string(), "  ".into(), "First".into(), "•Third".into(), "-".into()];
        assert_eq!(clean_items(&items), vec!["First", "Second", "Third"]);
        let kept = vec!["-5% churn".to_string(), "**Shipped** the page".into(), "- - nested".into(), "*emphasis*".into()];
        assert_eq!(clean_items(&kept), vec!["-5% churn", "**Shipped** the page", "- nested", "*emphasis*"]);
    }
}
