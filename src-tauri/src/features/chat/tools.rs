//! Read-only tools the chat model can call, adapted to Daytrace's SQLite schema.
//!
//! `fetchTimeline` and `fetchObservations` port Dayflow's dashboard tools
//! (GeminiDirectProvider+DashboardChat.swift / +DashboardTools.swift, MIT, © Jerry Liu): same
//! names, arguments, validation messages and result shape, with card ids added so answers can
//! cite cards. `searchCards`, `timeByCategory`, `daySummary` and `getCardDetails` are Daytrace
//! additions. Results are JSON strings kept under `RESULT_BUDGET_CHARS` so a request stays far
//! below the 90k-token ceiling.

use crate::db::{Db, StoredCard, SYSTEM_CATEGORY};
use crate::llm::chat_api::ToolSpec;
use crate::timeutil::{fmt_clock, local, logical_day, logical_day_bounds, now_ts};
use chrono::NaiveDate;
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, HashMap};

/// Soft size limit of one tool result (about 11k tokens).
pub const RESULT_BUDGET_CHARS: usize = 40_000;
/// Hard cap applied by the loop to any tool result.
pub const RESULT_MAX_CHARS: usize = 60_000;
/// Range used by searchCards when none is given.
const SEARCH_DEFAULT_DAYS: i64 = 90;

pub struct ToolContext {
    pub db: Db,
    /// Observations (screen transcriptions) are deleted after this many days; cards stay.
    pub retention_days: u32,
    pub capture_interval_secs: u64,
    pub idle_category: String,
}

#[derive(Debug, Clone)]
pub struct ToolOutput {
    /// JSON text sent to the model.
    pub content: String,
    /// One line for the UI ("Fetched 12 timeline cards for Thu, Oct 16.").
    pub summary: String,
    pub ok: bool,
    /// Card ids that appeared in the result (allowed as citations).
    pub card_ids: Vec<i64>,
}

impl ToolOutput {
    fn error(code: &str, message: String) -> ToolOutput {
        ToolOutput {
            content: json!({"summary": message, "error": {"code": code, "message": message}}).to_string(),
            summary: message,
            ok: false,
            card_ids: vec![],
        }
    }
}

pub fn specs() -> Vec<ToolSpec> {
    let range_props = |extra: Value| {
        let mut p = json!({
            "date": {"type": "string", "description": "Single day in YYYY-MM-DD format."},
            "startDate": {"type": "string", "description": "Range start date in YYYY-MM-DD."},
            "endDate": {"type": "string", "description": "Range end date in YYYY-MM-DD."},
        });
        if let (Some(p), Some(e)) = (p.as_object_mut(), extra.as_object()) {
            for (k, v) in e {
                p.insert(k.clone(), v.clone());
            }
        }
        p
    };
    vec![
        ToolSpec {
            name: "fetchTimeline",
            // Dayflow's description, plus the id sentence.
            description: "Fetch timeline cards for a single day or date range. Returns structured JSON cards including day, time range, title, summary, category, and optional detailed summaries. Each card has an id you can cite.",
            parameters: json!({
                "type": "object",
                "properties": range_props(json!({
                    "includeDetailedSummary": {"type": "boolean", "description": "When true (default), include detailedSummary. Set false for very large windows."},
                    "limit": {"type": "number", "description": "Optional row cap. If omitted, returns all matching rows."},
                })),
            }),
        },
        ToolSpec {
            name: "fetchObservations",
            description: "Fetch raw observations for a single day or date range. Returns structured JSON grouped by day, with each day's observations ordered chronologically.",
            parameters: json!({
                "type": "object",
                "properties": range_props(json!({
                    "limit": {"type": "number", "description": "Optional row cap. If omitted, returns all matching rows."},
                })),
            }),
        },
        ToolSpec {
            name: "searchCards",
            description: "Find timeline cards whose title, summary, detailed summary, subcategory or apps mention any of the given terms (case-insensitive, literal text). Include synonyms yourself, e.g. [\"Twitter\", \"x.com\"]. Defaults to the last 90 days when no date or range is given.",
            parameters: json!({
                "type": "object",
                "properties": range_props(json!({
                    "terms": {"type": "array", "items": {"type": "string"}, "description": "Words or phrases; a card matches if it contains any of them."},
                    "limit": {"type": "number", "description": "Optional row cap (default 50)."},
                })),
                "required": ["terms"],
            }),
        },
        ToolSpec {
            name: "timeByCategory",
            description: "Add up tracked time per category for a single day or date range, from card durations (failed batches excluded). Optionally broken down per day.",
            parameters: json!({
                "type": "object",
                "properties": range_props(json!({
                    "groupByDay": {"type": "boolean", "description": "When true, also return totals for each day."},
                })),
            }),
        },
        ToolSpec {
            name: "daySummary",
            description: "Overview of one day: first and last activity, tracked and idle time, time per category, the longest cards, and the apps seen most often in screenshots.",
            parameters: json!({
                "type": "object",
                "properties": {"date": {"type": "string", "description": "Single day in YYYY-MM-DD format."}},
                "required": ["date"],
            }),
        },
        ToolSpec {
            name: "getCardDetails",
            description: "Everything stored for one card by id: detailed summary, distractions, apps, the observations it was written from, and the window titles seen while it ran.",
            parameters: json!({
                "type": "object",
                "properties": {"id": {"type": "integer", "description": "Card id from another tool's result."}},
                "required": ["id"],
            }),
        },
    ]
}

/// Plain label for the UI while a tool runs (Dayflow: "Fetching timeline").
pub fn label(name: &str, input: &Value) -> String {
    let base = match name {
        "fetchTimeline" => "Fetching timeline",
        "fetchObservations" => "Fetching observations",
        "searchCards" => "Searching cards",
        "timeByCategory" => "Adding up time per category",
        "daySummary" => "Summarizing the day",
        "getCardDetails" => "Opening a card",
        _ => "Running a tool",
    };
    match parse_range(input) {
        Ok(r) => format!("{base} for {}", describe_range(&r)),
        Err(_) => base.to_string(),
    }
}

pub fn execute(ctx: &ToolContext, name: &str, input: &Value) -> ToolOutput {
    let result = match name {
        "fetchTimeline" => fetch_timeline(ctx, input),
        "fetchObservations" => fetch_observations(ctx, input),
        "searchCards" => search_cards(ctx, input),
        "timeByCategory" => time_by_category(ctx, input),
        "daySummary" => day_summary(ctx, input),
        "getCardDetails" => card_details(ctx, input),
        _ => return ToolOutput::error("unknown_tool", format!("Unknown tool '{name}'.")),
    };
    match result {
        Ok(out) => out,
        Err(ToolError::Validation(m)) => ToolOutput::error("validation_error", m),
        Err(ToolError::Db(m)) => ToolOutput::error("storage_error", format!("Could not read the timeline database: {m}")),
    }
}

enum ToolError {
    Validation(String),
    Db(String),
}

impl From<anyhow::Error> for ToolError {
    fn from(e: anyhow::Error) -> Self {
        ToolError::Db(format!("{e:#}"))
    }
}
impl From<rusqlite::Error> for ToolError {
    fn from(e: rusqlite::Error) -> Self {
        ToolError::Db(e.to_string())
    }
}

type R = Result<ToolOutput, ToolError>;

// ---- argument parsing (ports Dayflow's stringArg / boolArg / positiveIntArg) ----

fn string_arg(v: Option<&Value>) -> Option<String> {
    v.and_then(|v| v.as_str()).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

fn bool_arg(v: Option<&Value>) -> Option<bool> {
    match v? {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => n.as_f64().map(|f| f != 0.0),
        Value::String(s) => match s.to_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn positive_int_arg(v: Option<&Value>) -> Option<usize> {
    let n = match v? {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => s.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    (n >= 1.0).then_some(n as usize)
}

#[derive(Debug, Clone, PartialEq)]
pub struct DateRange {
    /// "date" or "range"
    pub mode: &'static str,
    pub date: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub from: i64,
    pub to: i64,
}

fn day_bounds(day: &str) -> Result<(i64, i64), ToolError> {
    if NaiveDate::parse_from_str(day, "%Y-%m-%d").is_err() {
        return Err(ToolError::Validation(format!("Invalid date format '{day}'. Use YYYY-MM-DD.")));
    }
    logical_day_bounds(day).ok_or_else(|| ToolError::Validation(format!("Invalid date format '{day}'. Use YYYY-MM-DD.")))
}

/// Dayflow's parseDashboardDateRange: either {date} or {startDate, endDate}; days start at 4 AM.
pub fn parse_range(args: &Value) -> Result<DateRange, String> {
    parse_range_inner(args).map_err(|e| match e {
        ToolError::Validation(m) | ToolError::Db(m) => m,
    })
}

fn parse_range_inner(args: &Value) -> Result<DateRange, ToolError> {
    let date = string_arg(args.get("date"));
    let start = string_arg(args.get("startDate"));
    let end = string_arg(args.get("endDate"));
    const COMBO: &str = "Provide either {date} OR {startDate, endDate}.";
    if let Some(date) = date {
        if start.is_some() || end.is_some() {
            return Err(ToolError::Validation(COMBO.into()));
        }
        let (from, to) = day_bounds(&date)?;
        return Ok(DateRange { mode: "date", date: Some(date), start_date: None, end_date: None, from, to });
    }
    let (Some(start), Some(end)) = (start, end) else { return Err(ToolError::Validation(COMBO.into())) };
    let (from, _) = day_bounds(&start)?;
    let (end_from, to) = day_bounds(&end)?;
    if from > end_from {
        return Err(ToolError::Validation("startDate must be less than or equal to endDate.".into()));
    }
    Ok(DateRange { mode: "range", date: None, start_date: Some(start), end_date: Some(end), from, to })
}

fn has_range_args(args: &Value) -> bool {
    ["date", "startDate", "endDate"].iter().any(|k| string_arg(args.get(*k)).is_some())
}

fn fmt_date(day: &str, pattern: &str) -> String {
    NaiveDate::parse_from_str(day, "%Y-%m-%d").map(|d| d.format(pattern).to_string()).unwrap_or_else(|_| day.to_string())
}

/// Dayflow's dashboardFetchDateDescription: "Thu, Oct 16" or "Oct 10 to Oct 16".
pub fn describe_range(r: &DateRange) -> String {
    if let Some(d) = &r.date {
        return fmt_date(d, "%a, %b %-d");
    }
    match (&r.start_date, &r.end_date) {
        (Some(s), Some(e)) => {
            let (a, b) = (fmt_date(s, "%b %-d"), fmt_date(e, "%b %-d"));
            if a == b { a } else { format!("{a} to {b}") }
        }
        _ => "the requested dates".into(),
    }
}

fn request_json(r: &DateRange, extra: Value) -> Value {
    let mut v = json!({"mode": r.mode, "date": r.date, "startDate": r.start_date, "endDate": r.end_date});
    if let (Some(m), Some(e)) = (v.as_object_mut(), extra.as_object()) {
        for (k, val) in e {
            m.insert(k.clone(), val.clone());
        }
    }
    v
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn minutes(secs: i64) -> i64 {
    (secs.max(0) as f64 / 60.0).round() as i64
}

fn is_failed(c: &StoredCard) -> bool {
    c.category == SYSTEM_CATEGORY
}

fn short(s: &str, n: usize) -> String {
    crate::llm::client::truncate(s, n)
}

fn json_len(v: &Value) -> usize {
    v.to_string().len()
}

/// Card fields shared by the tools.
fn card_item(c: &StoredCard, summary: Option<&str>) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("id".into(), json!(c.id));
    m.insert("day".into(), json!(c.day));
    m.insert("startTime".into(), json!(fmt_clock(c.start_ts)));
    m.insert("endTime".into(), json!(fmt_clock(c.end_ts)));
    m.insert("durationMinutes".into(), json!(minutes(c.end_ts - c.start_ts)));
    m.insert("title".into(), json!(c.title));
    if let Some(s) = summary {
        m.insert("summary".into(), json!(s));
    }
    m.insert("category".into(), json!(c.category));
    if !c.subcategory.is_empty() {
        m.insert("subcategory".into(), json!(c.subcategory));
    }
    m
}

/// Shrinks card items until they fit the budget: detailed summaries first, then summaries
/// shortened, then summaries dropped, then the list cut. Returns a note when anything was cut.
fn fit_items(items: &mut Vec<Map<String, Value>>, budget: usize, what: &str) -> Option<String> {
    let size = |items: &Vec<Map<String, Value>>| items.iter().map(|m| Value::Object(m.clone()).to_string().len() + 1).sum::<usize>();
    let mut notes = Vec::new();
    if size(items) > budget && items.iter().any(|m| m.contains_key("detailedSummary")) {
        items.iter_mut().for_each(|m| {
            m.remove("detailedSummary");
        });
        notes.push("Detailed summaries were omitted due to payload size.".to_string());
    }
    if size(items) > budget {
        for m in items.iter_mut() {
            if let Some(Value::String(s)) = m.get("summary") {
                let cut = short(s, 160);
                m.insert("summary".into(), json!(cut));
            }
        }
        notes.push("Summaries were shortened.".to_string());
    }
    if size(items) > budget {
        items.iter_mut().for_each(|m| {
            m.remove("summary");
        });
        notes.push("Summaries were omitted; use a narrower range or getCardDetails for detail.".to_string());
    }
    if size(items) > budget {
        let total = items.len();
        let mut used = 0;
        let mut keep = 0;
        for m in items.iter() {
            used += Value::Object(m.clone()).to_string().len() + 1;
            if used > budget {
                break;
            }
            keep += 1;
        }
        items.truncate(keep);
        notes.push(format!("Showing the first {keep} of {total} {what}. Narrow the date range or set a limit to see the rest."));
    }
    (!notes.is_empty()).then(|| notes.join(" "))
}

// ---- fetchTimeline ----

fn fetch_timeline(ctx: &ToolContext, args: &Value) -> R {
    let include_detailed = bool_arg(args.get("includeDetailedSummary")).unwrap_or(true);
    let limit = positive_int_arg(args.get("limit"));
    let range = parse_range_inner(args)?;
    let all = match &range.date {
        Some(d) => ctx.db.cards_for_day(d)?,
        None => ctx.db.cards_in_range(range.from, range.to, false)?,
    };
    let failed = all.iter().filter(|c| is_failed(c)).count();
    let mut cards: Vec<&StoredCard> = all.iter().filter(|c| !is_failed(c)).collect();
    if let Some(l) = limit {
        cards.truncate(l);
    }
    let mut items: Vec<Map<String, Value>> = cards
        .iter()
        .map(|c| {
            let idle = c.is_idle_shortcut;
            let mut m = card_item(c, (!idle).then_some(c.summary.as_str()));
            m.insert("distractionsCount".into(), json!(c.distractions.len()));
            if let Some(a) = &c.app_sites {
                m.insert("appSites".into(), json!({"primary": a.primary, "secondary": a.secondary}));
            }
            if include_detailed && !idle && !c.detailed_summary.is_empty() {
                m.insert("detailedSummary".into(), json!(c.detailed_summary));
            }
            m
        })
        .collect();
    let note = fit_items(&mut items, RESULT_BUDGET_CHARS, "cards");
    let ids: Vec<i64> = items.iter().filter_map(|m| m["id"].as_i64()).collect();
    let mut summary = format!("Fetched {} for {}.", plural(items.len(), "timeline card", "timeline cards"), describe_range(&range));
    if let Some(n) = &note {
        summary.push(' ');
        summary.push_str(n);
    }
    let mut out = json!({
        "request": request_json(&range, json!({"includeDetailedSummary": include_detailed, "limit": limit})),
        "summary": summary,
        "itemCount": items.len(),
        "truncated": note.is_some(),
        "items": items,
    });
    if failed > 0 {
        out["failedBatches"] = json!(failed);
        out["failedNote"] = json!("Some periods could not be analyzed (Processing failed); there are no cards for them.");
    }
    if items_empty(&out) {
        out["note"] = json!("No timeline cards found. Recording may not have been active on these days.");
    }
    Ok(ToolOutput { content: out.to_string(), summary, ok: true, card_ids: ids })
}

fn items_empty(v: &Value) -> bool {
    v["items"].as_array().is_none_or(|a| a.is_empty())
}

// ---- fetchObservations ----

fn fetch_observations(ctx: &ToolContext, args: &Value) -> R {
    let limit = positive_int_arg(args.get("limit"));
    let range = parse_range_inner(args)?;
    let mut obs = ctx.db.observations_in_range(range.from, range.to)?;
    if let Some(l) = limit {
        obs.truncate(l);
    }
    let total = obs.len();
    // Dayflow's dashboardObservationDayGroups: consecutive observations grouped by logical day.
    let mut used = 0;
    let mut kept = 0;
    let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
    for o in &obs {
        let item = json!({"startTime": fmt_clock(o.start_ts), "endTime": fmt_clock(o.end_ts), "observation": o.text});
        used += json_len(&item) + 1;
        if used > RESULT_BUDGET_CHARS {
            break;
        }
        kept += 1;
        let day = logical_day(o.start_ts);
        match groups.last_mut() {
            Some((d, items)) if *d == day => items.push(item),
            _ => groups.push((day, vec![item])),
        }
    }
    let day_count = groups.len();
    let mut summary = format!("Fetched {} for {}", plural(kept, "observation", "observations"), describe_range(&range));
    if day_count > 0 {
        summary.push_str(&format!(" across {}", plural(day_count, "day", "days")));
    }
    summary.push('.');
    let truncated = kept < total;
    if truncated {
        summary.push_str(&format!(" Showing the first {kept} of {total}; narrow the range for the rest."));
    }
    let mut out = json!({
        "request": request_json(&range, json!({"limit": limit})),
        "summary": summary,
        "dayCount": day_count,
        "itemCount": kept,
        "truncated": truncated,
        "items": groups.into_iter().map(|(day, observations)| json!({"day": day, "observations": observations})).collect::<Vec<_>>(),
    });
    if total == 0 {
        let retention_start = now_ts() - ctx.retention_days as i64 * 86400;
        out["note"] = json!(if range.to <= retention_start {
            format!("Observations are kept for {} days and these days are older. Timeline cards are kept; use fetchTimeline instead.", ctx.retention_days)
        } else {
            "No observations found. Recording may not have been active on these days.".to_string()
        });
    }
    Ok(ToolOutput { content: out.to_string(), summary, ok: true, card_ids: vec![] })
}

// ---- searchCards ----

fn search_cards(ctx: &ToolContext, args: &Value) -> R {
    let mut terms: Vec<String> = match args.get("terms") {
        Some(Value::Array(a)) => a.iter().filter_map(|t| t.as_str()).map(|t| t.trim().to_string()).collect(),
        Some(Value::String(s)) => s.split(',').map(|t| t.trim().to_string()).collect(),
        _ => string_arg(args.get("query")).map(|q| q.split(',').map(|t| t.trim().to_string()).collect()).unwrap_or_default(),
    };
    terms.retain(|t| !t.is_empty());
    terms.dedup();
    if terms.is_empty() {
        return Err(ToolError::Validation("Provide terms: a list of words or phrases to search for.".into()));
    }
    let limit = positive_int_arg(args.get("limit")).unwrap_or(50);
    let range = if has_range_args(args) {
        parse_range_inner(args)?
    } else {
        let today = logical_day(now_ts());
        let start = NaiveDate::parse_from_str(&today, "%Y-%m-%d")
            .map(|d| (d - chrono::Duration::days(SEARCH_DEFAULT_DAYS - 1)).format("%Y-%m-%d").to_string())
            .unwrap_or_else(|_| today.clone());
        parse_range_inner(&json!({"startDate": start, "endDate": today}))?
    };
    let needles: Vec<String> = terms.iter().map(|t| t.to_lowercase()).collect();
    let cards = ctx.db.cards_in_range(range.from, range.to, true)?;
    let mut matches: Vec<(&StoredCard, Vec<String>)> = Vec::new();
    for c in &cards {
        let mut hay = format!("{}\n{}\n{}\n{}", c.title, c.summary, c.detailed_summary, c.subcategory);
        if let Some(a) = &c.app_sites {
            for s in [&a.primary, &a.secondary].into_iter().flatten() {
                hay.push('\n');
                hay.push_str(s);
            }
        }
        let hay = hay.to_lowercase();
        let hit: Vec<String> = terms.iter().zip(&needles).filter(|(_, n)| hay.contains(n.as_str())).map(|(t, _)| t.clone()).collect();
        if !hit.is_empty() {
            matches.push((c, hit));
        }
    }
    let match_count = matches.len();
    let total_minutes: i64 = matches.iter().map(|(c, _)| minutes(c.end_ts - c.start_ts)).sum();
    let mut items: Vec<Map<String, Value>> = matches
        .iter()
        .take(limit)
        .map(|(c, hit)| {
            let mut m = card_item(c, Some(c.summary.as_str()));
            m.insert("matched".into(), json!(hit));
            m
        })
        .collect();
    let mut note = fit_items(&mut items, RESULT_BUDGET_CHARS, "matching cards");
    if match_count > limit && note.is_none() {
        note = Some(format!("Showing the first {limit} of {match_count} matching cards."));
    }
    let ids: Vec<i64> = items.iter().filter_map(|m| m["id"].as_i64()).collect();
    let summary = format!(
        "Found {} mentioning {} in {}.",
        plural(match_count, "card", "cards"),
        terms.iter().map(|t| format!("\"{t}\"")).collect::<Vec<_>>().join(" or "),
        describe_range(&range)
    );
    let out = json!({
        "request": request_json(&range, json!({"terms": terms, "limit": limit})),
        "summary": summary,
        "matchCount": match_count,
        "totalMinutes": total_minutes,
        "truncated": note.is_some(),
        "note": note,
        "items": items,
    });
    Ok(ToolOutput { content: out.to_string(), summary, ok: true, card_ids: ids })
}

// ---- timeByCategory ----

fn category_totals(cards: &[StoredCard], from: i64, to: i64) -> (i64, Vec<(String, i64)>) {
    let mut secs: HashMap<String, i64> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    for c in cards.iter().filter(|c| !is_failed(c)) {
        let s = c.start_ts.max(from);
        let e = c.end_ts.min(to);
        if e <= s {
            continue;
        }
        if !secs.contains_key(&c.category) {
            order.push(c.category.clone());
        }
        *secs.entry(c.category.clone()).or_default() += e - s;
    }
    let total: i64 = secs.values().sum();
    let mut list: Vec<(String, i64)> = order.into_iter().map(|k| (k.clone(), secs[&k])).collect();
    list.sort_by(|a, b| b.1.cmp(&a.1));
    (total, list)
}

fn totals_json(total: i64, list: &[(String, i64)]) -> Value {
    json!(list
        .iter()
        .map(|(c, s)| json!({"category": c, "minutes": minutes(*s), "share": if total > 0 { (*s as f64 * 100.0 / total as f64).round() as i64 } else { 0 }}))
        .collect::<Vec<_>>())
}

fn time_by_category(ctx: &ToolContext, args: &Value) -> R {
    let by_day = bool_arg(args.get("groupByDay")).unwrap_or(false);
    let range = parse_range_inner(args)?;
    let cards = ctx.db.cards_in_range(range.from, range.to, true)?;
    let (total, list) = category_totals(&cards, range.from, range.to);
    let mut out = json!({
        "request": request_json(&range, json!({"groupByDay": by_day})),
        "totalMinutes": minutes(total),
        "categories": totals_json(total, &list),
    });
    if by_day {
        let mut days: BTreeMap<String, Vec<StoredCard>> = BTreeMap::new();
        for c in &cards {
            days.entry(c.day.clone()).or_default().push(c.clone());
        }
        out["days"] = json!(days
            .iter()
            .map(|(d, cs)| {
                let (b_from, b_to) = logical_day_bounds(d).unwrap_or((range.from, range.to));
                let (t, l) = category_totals(cs, b_from.max(range.from), b_to.min(range.to));
                json!({"day": d, "totalMinutes": minutes(t), "categories": totals_json(t, &l)})
            })
            .collect::<Vec<_>>());
    }
    let summary = format!("Added up {} of tracked time for {}.", fmt_minutes(minutes(total)), describe_range(&range));
    out["summary"] = json!(summary);
    if total == 0 {
        out["note"] = json!("No timeline cards found. Recording may not have been active on these days.");
    }
    Ok(ToolOutput { content: out.to_string(), summary, ok: true, card_ids: vec![] })
}

fn fmt_minutes(m: i64) -> String {
    match (m / 60, m % 60) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

// ---- daySummary ----

fn app_counts(db: &Db, from: i64, to: i64) -> Result<(Vec<(String, i64)>, i64), rusqlite::Error> {
    // Rows of private apps (hidden_app set) and away time (skip_reason) are not named.
    let c = db.conn();
    let mut st = c.prepare(
        "SELECT app_name, COUNT(*) FROM screenshots WHERE captured_at >= ? AND captured_at < ?
         AND skip_reason IS NULL AND hidden_app IS NULL AND app_name IS NOT NULL AND app_name != ''
         GROUP BY app_name ORDER BY COUNT(*) DESC LIMIT 8",
    )?;
    let apps = st.query_map([from, to], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?.collect::<Result<Vec<_>, _>>()?;
    let private: i64 = c.query_row(
        "SELECT COUNT(*) FROM screenshots WHERE captured_at >= ? AND captured_at < ? AND hidden_app IS NOT NULL",
        [from, to],
        |r| r.get(0),
    )?;
    Ok((apps, private))
}

fn day_summary(ctx: &ToolContext, args: &Value) -> R {
    let Some(date) = string_arg(args.get("date")) else {
        return Err(ToolError::Validation("Provide date in YYYY-MM-DD format.".into()));
    };
    let range = parse_range_inner(&json!({"date": date}))?;
    let cards = ctx.db.cards_for_day(&date)?;
    let failed = cards.iter().filter(|c| is_failed(c)).count();
    let live: Vec<&StoredCard> = cards.iter().filter(|c| !is_failed(c)).collect();
    let idle_name = ctx.idle_category.to_lowercase();
    let is_idle = |c: &StoredCard| c.is_idle_shortcut || c.category.to_lowercase() == idle_name;
    let idle_secs: i64 = live.iter().filter(|c| is_idle(c)).map(|c| c.end_ts - c.start_ts).sum();
    let active_secs: i64 = live.iter().filter(|c| !is_idle(c)).map(|c| c.end_ts - c.start_ts).sum();
    let (total, list) = category_totals(&cards, range.from, range.to);
    let mut longest: Vec<&&StoredCard> = live.iter().filter(|c| !is_idle(c)).collect();
    longest.sort_by_key(|c| -(c.end_ts - c.start_ts));
    let longest_json: Vec<Value> = longest.iter().take(5).map(|c| Value::Object(card_item(c, None))).collect();
    let ids: Vec<i64> = longest.iter().take(5).map(|c| c.id).collect();
    let (apps, private) = app_counts(&ctx.db, range.from, range.to)?;
    let per_shot = ctx.capture_interval_secs.max(1) as i64;
    let first = live.iter().filter(|c| !is_idle(c)).map(|c| c.start_ts).min();
    let last = live.iter().filter(|c| !is_idle(c)).map(|c| c.end_ts).max();
    let summary = format!("Summarized {}: {} active across {}.", describe_range(&range), fmt_minutes(minutes(active_secs)), plural(live.len(), "card", "cards"));
    let mut out = json!({
        "day": date,
        "summary": summary,
        "cardCount": live.len(),
        "firstActivity": first.map(fmt_clock),
        "lastActivity": last.map(fmt_clock),
        "activeMinutes": minutes(active_secs),
        "idleMinutes": minutes(idle_secs),
        "categories": totals_json(total, &list),
        "longestCards": longest_json,
        "topApps": apps.iter().map(|(a, n)| json!({"app": a, "approxMinutes": minutes(n * per_shot)})).collect::<Vec<_>>(),
    });
    if private > 0 {
        out["privateAppMinutes"] = json!(minutes(private * per_shot));
    }
    if failed > 0 {
        out["failedBatches"] = json!(failed);
    }
    if live.is_empty() {
        out["note"] = json!("No timeline cards found. Recording may not have been active on this day.");
    }
    Ok(ToolOutput { content: out.to_string(), summary, ok: true, card_ids: ids })
}

// ---- getCardDetails ----

fn window_titles(db: &Db, from: i64, to: i64) -> Result<Vec<(String, String, i64)>, rusqlite::Error> {
    let c = db.conn();
    let mut st = c.prepare(
        "SELECT coalesce(app_name, ''), window_title, COUNT(*) FROM screenshots WHERE captured_at >= ? AND captured_at < ?
         AND skip_reason IS NULL AND hidden_app IS NULL AND window_title IS NOT NULL AND window_title != ''
         GROUP BY app_name, window_title ORDER BY COUNT(*) DESC LIMIT 12",
    )?;
    let rows = st.query_map([from, to], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn card_details(ctx: &ToolContext, args: &Value) -> R {
    let id = match args.get("id") {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    };
    let Some(id) = id else { return Err(ToolError::Validation("Provide id: the card's numeric id.".into())) };
    let Some(card) = ctx.db.card(id)? else {
        return Err(ToolError::Validation(format!("No card with id {id}.")));
    };
    let mut item = card_item(&card, Some(card.summary.as_str()));
    let mut note = None;
    if is_deleted(&ctx.db, id)? {
        let newer: Vec<i64> = ctx.db.cards_in_range(card.start_ts, card.end_ts, true)?.iter().map(|c| c.id).collect();
        note = Some(format!("This card was replaced by a newer analysis. Current cards for this time: {newer:?}."));
    }
    if !card.detailed_summary.is_empty() {
        item.insert("detailedSummary".into(), json!(card.detailed_summary));
    }
    if !card.distractions.is_empty() {
        item.insert(
            "distractions".into(),
            json!(card.distractions.iter().map(|d| json!({"startTime": d.start_time, "endTime": d.end_time, "title": d.title, "summary": d.summary})).collect::<Vec<_>>()),
        );
    }
    if let Some(a) = &card.app_sites {
        item.insert("appSites".into(), json!({"primary": a.primary, "secondary": a.secondary}));
    }
    let titles = window_titles(&ctx.db, card.start_ts, card.end_ts)?;
    let per_shot = ctx.capture_interval_secs.max(1) as i64;
    item.insert(
        "windowTitles".into(),
        json!(titles.iter().map(|(a, t, n)| json!({"app": a, "title": short(t, 140), "approxMinutes": minutes(n * per_shot)})).collect::<Vec<_>>()),
    );
    // Observations fill the rest of the budget.
    let base = json_len(&Value::Object(item.clone()));
    let mut used = base;
    let mut observations = Vec::new();
    let all_obs = ctx.db.observations_in_range(card.start_ts, card.end_ts)?;
    for o in &all_obs {
        let v = json!({"startTime": fmt_clock(o.start_ts), "endTime": fmt_clock(o.end_ts), "observation": o.text});
        used += json_len(&v) + 1;
        if used > RESULT_BUDGET_CHARS {
            break;
        }
        observations.push(v);
    }
    if all_obs.is_empty() && card.end_ts < now_ts() - ctx.retention_days as i64 * 86400 {
        item.insert("observationsNote".into(), json!(format!("Observations are kept for {} days; this card is older.", ctx.retention_days)));
    }
    item.insert("observations".into(), json!(observations));
    if let Some(n) = note {
        item.insert("note".into(), json!(n));
    }
    let when = local(card.start_ts).format("%a, %b %-d").to_string();
    let summary = format!("Opened \"{}\" ({when}, {} to {}).", short(&card.title, 60), fmt_clock(card.start_ts), fmt_clock(card.end_ts));
    item.insert("summary".into(), json!(summary));
    Ok(ToolOutput { content: Value::Object(item).to_string(), summary, ok: true, card_ids: vec![id] })
}

fn is_deleted(db: &Db, id: i64) -> Result<bool, rusqlite::Error> {
    db.conn().query_row("SELECT is_deleted FROM timeline_cards WHERE id = ?", [id], |r| r.get::<_, i64>(0)).map(|d| d != 0)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::db::ResolvedCard;
    use crate::types::{ActivityCard, AppSites, Observation};
    use chrono::{Local, TimeZone};

    pub fn ts(day: u32, h: u32, m: u32) -> i64 {
        Local.with_ymd_and_hms(2026, 10, day, h, m, 0).unwrap().timestamp()
    }

    fn card(title: &str, cat: &str, s: i64, e: i64, summary: &str) -> ResolvedCard {
        ResolvedCard {
            start_ts: s,
            end_ts: e,
            card: ActivityCard {
                start_time: fmt_clock(s),
                end_time: fmt_clock(e),
                category: cat.into(),
                subcategory: String::new(),
                title: title.into(),
                summary: summary.into(),
                detailed_summary: format!("Detailed: {summary}"),
                distractions: None,
                app_sites: Some(AppSites { primary: Some("code.visualstudio.com".into()), secondary: None }),
            },
            idle_meta: None,
        }
    }

    /// A database with a batch, three cards and two observations on 2026-10-16.
    pub fn ctx() -> ToolContext {
        let db = Db::memory();
        let base = ts(16, 9, 0);
        db.insert_screenshot(base, "", 0, 0, Some("Code.exe"), Some("main.rs - daytrace"), None, None).unwrap();
        db.insert_screenshot(base + 10, "", 0, 0, Some("Code.exe"), Some("main.rs - daytrace"), None, None).unwrap();
        db.insert_screenshot(base + 20, "", 0, 0, Some("KeePass.exe"), None, Some("KeePass.exe"), None).unwrap();
        let shots = db.unbatched_screenshots(0).unwrap();
        let b = db.create_batch(&shots).unwrap();
        let cards = vec![
            card("Writing the chat feature", "Work", ts(16, 9, 0), ts(16, 10, 30), "Rust code for chat tools in VS Code"),
            card("Scrolling Twitter", "Distraction", ts(16, 10, 30), ts(16, 11, 0), "Reading threads on X"),
            card("Lunch break", "Idle", ts(16, 12, 0), ts(16, 13, 0), "Away"),
            card("Late debugging", "Work", ts(17, 1, 0), ts(17, 1, 30), "Still on 2026-10-16 (before 4 AM)"),
        ];
        db.replace_cards_in_range(ts(16, 4, 0), ts(17, 4, 0), &cards, b, false).unwrap();
        db.insert_observations(
            b,
            &[
                Observation { start_ts: ts(16, 9, 0), end_ts: ts(16, 9, 5), text: "Editing chat_api.rs".into() },
                Observation { start_ts: ts(16, 10, 35), end_ts: ts(16, 10, 40), text: "Reading a thread on x.com".into() },
            ],
            Some("m"),
        )
        .unwrap();
        ToolContext { db, retention_days: 7, capture_interval_secs: 10, idle_category: "Idle".into() }
    }

    fn parse(o: &ToolOutput) -> Value {
        serde_json::from_str(&o.content).unwrap()
    }

    #[test]
    fn date_range_validation_matches_dayflow() {
        assert_eq!(parse_range(&json!({})).unwrap_err(), "Provide either {date} OR {startDate, endDate}.");
        assert_eq!(parse_range(&json!({"date": "2026-10-16", "endDate": "2026-10-17"})).unwrap_err(), "Provide either {date} OR {startDate, endDate}.");
        assert_eq!(parse_range(&json!({"date": "16/10/2026"})).unwrap_err(), "Invalid date format '16/10/2026'. Use YYYY-MM-DD.");
        assert_eq!(parse_range(&json!({"startDate": "2026-10-17", "endDate": "2026-10-16"})).unwrap_err(), "startDate must be less than or equal to endDate.");
        let r = parse_range(&json!({"startDate": "2026-10-10", "endDate": "2026-10-16"})).unwrap();
        assert_eq!((r.from, r.to), (ts(10, 4, 0), ts(17, 4, 0)));
        assert_eq!(describe_range(&r), "Oct 10 to Oct 16");
        assert_eq!(describe_range(&parse_range(&json!({"date": "2026-10-16"})).unwrap()), "Fri, Oct 16");
    }

    #[test]
    fn fetch_timeline_returns_cards_with_ids() {
        let c = ctx();
        let o = execute(&c, "fetchTimeline", &json!({"date": "2026-10-16"}));
        assert!(o.ok, "{}", o.content);
        let v = parse(&o);
        assert_eq!(v["itemCount"], 4, "logical day includes the 1 AM card");
        assert_eq!(v["items"][0]["title"], "Writing the chat feature");
        assert_eq!(v["items"][0]["durationMinutes"], 90);
        assert!(v["items"][0]["detailedSummary"].is_string());
        assert_eq!(o.card_ids.len(), 4);
        assert_eq!(o.summary, "Fetched 4 timeline cards for Fri, Oct 16.");
        let no_detail = parse(&execute(&c, "fetchTimeline", &json!({"date": "2026-10-16", "includeDetailedSummary": false, "limit": 1})));
        assert!(no_detail["items"][0].get("detailedSummary").is_none());
        assert_eq!(no_detail["itemCount"], 1);
        let empty = parse(&execute(&c, "fetchTimeline", &json!({"date": "2026-10-01"})));
        assert_eq!(empty["itemCount"], 0);
        assert!(empty["note"].is_string());
        let bad = execute(&c, "fetchTimeline", &json!({"date": "yesterday"}));
        assert!(!bad.ok);
        assert_eq!(parse(&bad)["error"]["code"], "validation_error");
    }

    #[test]
    fn fit_items_degrades_in_steps() {
        let mut items: Vec<Map<String, Value>> = (0..400)
            .map(|i| {
                let mut m = Map::new();
                m.insert("id".into(), json!(i));
                m.insert("summary".into(), json!("s".repeat(300)));
                m.insert("detailedSummary".into(), json!("d".repeat(800)));
                m
            })
            .collect();
        let note = fit_items(&mut items, 40_000, "cards").unwrap();
        assert!(note.contains("Detailed summaries were omitted"));
        assert!(items.iter().map(|m| Value::Object(m.clone()).to_string().len()).sum::<usize>() <= 40_000);
        assert!(items.iter().all(|m| !m.contains_key("detailedSummary")));
    }

    #[test]
    fn fetch_observations_grouped_by_day() {
        let c = ctx();
        let o = execute(&c, "fetchObservations", &json!({"date": "2026-10-16"}));
        let v = parse(&o);
        assert_eq!(v["dayCount"], 1);
        assert_eq!(v["items"][0]["day"], "2026-10-16");
        assert_eq!(v["items"][0]["observations"][1]["observation"], "Reading a thread on x.com");
        let old = parse(&execute(&c, "fetchObservations", &json!({"date": "2020-01-01"})));
        assert!(old["note"].as_str().unwrap().contains("kept for 7 days"));
    }

    #[test]
    fn search_matches_any_term_case_insensitively() {
        let c = ctx();
        let o = execute(&c, "searchCards", &json!({"terms": ["twitter", "X.COM"], "startDate": "2026-10-15", "endDate": "2026-10-17"}));
        let v = parse(&o);
        assert_eq!(v["matchCount"], 1, "{v}");
        assert_eq!(v["items"][0]["title"], "Scrolling Twitter");
        assert_eq!(v["totalMinutes"], 30);
        let apps = parse(&execute(&c, "searchCards", &json!({"terms": "visualstudio", "date": "2026-10-16"})));
        assert_eq!(apps["matchCount"], 4, "app sites are searched");
        assert!(!execute(&c, "searchCards", &json!({"terms": []})).ok);
    }

    #[test]
    fn time_by_category_clips_and_groups() {
        let c = ctx();
        let v = parse(&execute(&c, "timeByCategory", &json!({"date": "2026-10-16", "groupByDay": true})));
        assert_eq!(v["totalMinutes"], 90 + 30 + 60 + 30);
        assert_eq!(v["categories"][0]["category"], "Work");
        assert_eq!(v["categories"][0]["minutes"], 120);
        assert_eq!(v["days"][0]["day"], "2026-10-16");
        // A range ending on the 15th clips nothing in.
        let none = parse(&execute(&c, "timeByCategory", &json!({"startDate": "2026-10-14", "endDate": "2026-10-15"})));
        assert_eq!(none["totalMinutes"], 0);
    }

    #[test]
    fn day_summary_hides_private_apps() {
        let c = ctx();
        let o = execute(&c, "daySummary", &json!({"date": "2026-10-16"}));
        let v = parse(&o);
        assert_eq!(v["activeMinutes"], 150);
        assert_eq!(v["idleMinutes"], 60);
        assert_eq!(v["firstActivity"], "9:00 AM");
        assert_eq!(v["longestCards"][0]["title"], "Writing the chat feature");
        assert_eq!(v["topApps"][0]["app"], "Code.exe");
        assert!(!o.content.contains("KeePass"), "private app is not named");
        assert!(v["privateAppMinutes"].as_i64().is_some());
    }

    #[test]
    fn card_details_include_observations_and_titles() {
        let c = ctx();
        let id = c.db.cards_for_day("2026-10-16").unwrap()[0].id;
        let o = execute(&c, "getCardDetails", &json!({"id": id}));
        let v = parse(&o);
        assert_eq!(v["title"], "Writing the chat feature");
        assert_eq!(v["observations"][0]["observation"], "Editing chat_api.rs");
        assert_eq!(v["windowTitles"][0]["title"], "main.rs - daytrace");
        assert_eq!(o.card_ids, vec![id]);
        assert!(!execute(&c, "getCardDetails", &json!({"id": 99999})).ok);
        assert!(!execute(&c, "nope", &json!({})).ok);
    }

    #[test]
    fn labels_are_plain() {
        assert_eq!(label("fetchTimeline", &json!({"date": "2026-10-16"})), "Fetching timeline for Fri, Oct 16");
        assert_eq!(label("getCardDetails", &json!({"id": 3})), "Opening a card");
    }
}
