//! Read-only access to the Daytrace database for the CLI and the MCP server.
//!
//! Ported from Dayflow's tools/dayflow-cli (MIT, (c) 2025 Jerry Liu): Database.swift (read-only
//! connection), Queries.swift (SQL semantics: a day selects cards *starting* inside its 4 AM
//! window, a range selects cards overlapping it, both skip soft-deleted rows and order by
//! start_ts), JSONOut.swift (stable JSON with `schema_version`), DayBoundary.swift (4 AM days,
//! Monday 4 AM weeks) and Categories.swift (categories come from the app's settings, not SQLite).
//!
//! Nothing here reads screenshot file paths or the private-app column (`screenshots.hidden_app`);
//! the only screenshot data used is the time of the last capture.

use crate::timeutil::{local, logical_day, logical_day_bounds, now_ts, DAY_START_HOUR};
use crate::types::{default_categories, Category};
use chrono::{Datelike, Duration, NaiveDate};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: i64 = 1;
pub const DB_FILE: &str = "daytrace.sqlite";
/// Tauri's app data folder is named after the bundle identifier (tauri.conf.json).
pub const APP_IDENTIFIER: &str = "dev.daytrace.app";
/// Ranges longer than this return totals instead of activities (Dayflow's maxRangeDaysForActivities).
pub const MAX_RANGE_DAYS_FOR_ACTIVITIES: i64 = 7;
pub const SEARCH_LIMIT: usize = 50;
pub const SYSTEM_CATEGORY: &str = "System";

#[derive(Debug, Clone, PartialEq)]
pub enum QueryError {
    /// No database file: Daytrace is not installed or has not recorded yet.
    NoData(String),
    /// Bad input from the caller.
    Invalid(String),
    NotFound(String),
    Db(String),
}

impl QueryError {
    pub fn message(&self) -> &str {
        match self {
            QueryError::NoData(m) | QueryError::Invalid(m) | QueryError::NotFound(m) | QueryError::Db(m) => m,
        }
    }
    /// Stable code for JSON errors.
    pub fn code(&self) -> &'static str {
        match self {
            QueryError::NoData(_) => "no_data",
            QueryError::Invalid(_) => "invalid_input",
            QueryError::NotFound(_) => "not_found",
            QueryError::Db(_) => "database_error",
        }
    }
    /// CLI exit code (Dayflow: 1 unexpected, 2 bad arguments, 3 not found, 5 no data).
    pub fn exit_code(&self) -> i32 {
        match self {
            QueryError::NoData(_) => 5,
            QueryError::Invalid(_) => 2,
            QueryError::NotFound(_) => 3,
            QueryError::Db(_) => 1,
        }
    }
}

impl From<rusqlite::Error> for QueryError {
    fn from(e: rusqlite::Error) -> Self {
        QueryError::Db(format!("Query failed: {e}"))
    }
}

pub type QResult<T> = Result<T, QueryError>;

// ---- locating the data ----

/// Where the app keeps its data: %APPDATA%\dev.daytrace.app on Windows (Tauri's app_data_dir).
pub fn default_data_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join(APP_IDENTIFIER))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support").join(APP_IDENTIFIER))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .map(|d| d.join(APP_IDENTIFIER))
    }
}

/// The database to read and the folder that holds settings.json.
#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub db_path: PathBuf,
    pub data_dir: PathBuf,
}

impl Source {
    pub fn from_dir(dir: PathBuf) -> Source {
        Source { db_path: dir.join(DB_FILE), data_dir: dir }
    }

    /// `--db` (a database file, or a folder holding daytrace.sqlite) wins over DAYTRACE_DATA_DIR,
    /// which wins over the default data folder. Without any of them the database counts as
    /// missing, which every command reports as "no data".
    pub fn resolve(db_flag: Option<&str>, env_dir: Option<&str>) -> Source {
        if let Some(p) = db_flag.map(str::trim).filter(|p| !p.is_empty()) {
            let p = PathBuf::from(p);
            if p.is_dir() {
                return Source::from_dir(p);
            }
            let dir = p.parent().filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
            return Source { db_path: p, data_dir: dir };
        }
        if let Some(d) = env_dir.map(str::trim).filter(|d| !d.is_empty()) {
            return Source::from_dir(PathBuf::from(d));
        }
        Source::from_dir(default_data_dir().unwrap_or_default())
    }

    /// Opens the database read-only. Read-only is enforced by the connection (READ_ONLY flag and
    /// `PRAGMA query_only`), so a bug here cannot change the app's data. The app keeps the
    /// database in WAL mode, so reading while it writes is safe.
    pub fn open(&self) -> QResult<Connection> {
        if !self.db_path.is_file() {
            return Err(QueryError::NoData("No Daytrace data found. Open Daytrace and let it record for a bit.".into()));
        }
        let conn = Connection::open_with_flags(&self.db_path, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)
            .map_err(|e| QueryError::Db(format!("Could not open Daytrace's database: {e}")))?;
        conn.execute_batch("PRAGMA query_only = 1")?;
        // Don't fail instantly if the app is mid-checkpoint.
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        Ok(conn)
    }

    /// The user's categories from settings.json; the default set when absent or unreadable
    /// (what the app itself does on load).
    pub fn categories(&self) -> Vec<Category> {
        std::fs::read_to_string(self.data_dir.join("settings.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.get("categories").cloned())
            .and_then(|c| serde_json::from_value::<Vec<Category>>(c).ok())
            .map(|mut c| {
                c.retain(|c| !c.name.trim().is_empty());
                c
            })
            .filter(|c| !c.is_empty())
            .unwrap_or_else(default_categories)
    }

    pub fn idle_category(&self) -> String {
        self.categories().into_iter().find(|c| c.is_idle).map(|c| c.name).unwrap_or_else(|| "Idle".into())
    }
}

// ---- days and weeks ----

#[derive(Debug, Clone, PartialEq)]
pub struct DayWindow {
    pub day: String,
    pub start: i64,
    pub end: i64,
}

/// The Daytrace day for "YYYY-MM-DD", "today" or "yesterday" (4 AM to 4 AM).
pub fn day_window(key: &str) -> QResult<DayWindow> {
    let key = key.trim();
    let day = match key.to_ascii_lowercase().as_str() {
        "today" => logical_day(now_ts()),
        "yesterday" => shift_day(&logical_day(now_ts()), -1).unwrap_or_default(),
        _ => key.to_string(),
    };
    let invalid = || QueryError::Invalid(format!("\"{key}\" is not a date. Use YYYY-MM-DD, e.g. 2026-03-11."));
    let day = NaiveDate::parse_from_str(&day, "%Y-%m-%d").map_err(|_| invalid())?.format("%Y-%m-%d").to_string();
    let (start, end) = logical_day_bounds(&day).ok_or_else(invalid)?;
    Ok(DayWindow { day, start, end })
}

pub fn today_window() -> DayWindow {
    day_window("today").expect("today is a valid day")
}

pub fn shift_day(day: &str, delta: i64) -> Option<String> {
    let d = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    Some((d + Duration::days(delta)).format("%Y-%m-%d").to_string())
}

/// Number of days from the start of `from` to the end of `to`, both included.
pub fn days_between(from: &DayWindow, to: &DayWindow) -> i64 {
    let a = NaiveDate::parse_from_str(&from.day, "%Y-%m-%d").ok();
    let b = NaiveDate::parse_from_str(&to.day, "%Y-%m-%d").ok();
    match (a, b) {
        (Some(a), Some(b)) => (b - a).num_days() + 1,
        _ => 0,
    }
}

/// Parses --from/--to (or the MCP `from`/`to` arguments), from first.
pub fn range_windows(from: &str, to: &str) -> QResult<(DayWindow, DayWindow)> {
    let bad = || QueryError::Invalid("Ranges need from and to as YYYY-MM-DD, with from first.".into());
    let f = day_window(from).map_err(|_| bad())?;
    let t = day_window(to).map_err(|_| bad())?;
    if f.start > t.start {
        return Err(bad());
    }
    Ok((f, t))
}

/// Monday 4 AM to the next Monday 4 AM around the Daytrace day `day` (WeeklyDateRange.containing).
pub fn week_window(day: &DayWindow) -> QResult<(i64, i64)> {
    let d = NaiveDate::parse_from_str(&day.day, "%Y-%m-%d").map_err(|_| QueryError::Invalid("Invalid date. Use YYYY-MM-DD.".into()))?;
    let monday = d - Duration::days(d.weekday().num_days_from_monday() as i64);
    let next = monday + Duration::days(7);
    let s = logical_day_bounds(&monday.format("%Y-%m-%d").to_string());
    let e = logical_day_bounds(&next.format("%Y-%m-%d").to_string());
    match (s, e) {
        (Some((s, _)), Some((e, _))) => Ok((s, e)),
        _ => Err(QueryError::Invalid("Invalid date. Use YYYY-MM-DD.".into())),
    }
}

/// ISO 8601 with the local offset, e.g. 2026-03-11T09:30:00+01:00 (Dayflow's isoFormatter).
pub fn iso(ts: i64) -> String {
    local(ts).format("%Y-%m-%dT%H:%M:%S%:z").to_string()
}

/// IANA name of the local time zone. A TZ variable wins, as it does for chrono's local time.
pub fn time_zone() -> String {
    if let Some(tz) = std::env::var("TZ").ok().map(|t| t.trim_start_matches(':').to_string()).filter(|t| !t.is_empty()) {
        return tz;
    }
    iana_time_zone::get_timezone().unwrap_or_else(|_| local(now_ts()).format("UTC%:z").to_string())
}

// ---- activities ----

#[derive(Debug, Clone, PartialEq)]
pub struct Activity {
    pub record_id: i64,
    pub day: String,
    pub start: i64,
    pub end: i64,
    pub title: String,
    pub summary: String,
    pub detailed_summary: String,
    pub category: String,
    pub subcategory: String,
    pub apps: Vec<String>,
    pub distraction_count: usize,
}

impl Activity {
    pub fn duration_minutes(&self) -> i64 {
        (self.end - self.start).max(0) / 60
    }
}

const CARD_COLUMNS: &str = "id, day, start_ts, end_ts, title, summary, detailed_summary, category, subcategory, metadata";

fn activity_from_row(r: &rusqlite::Row) -> rusqlite::Result<Activity> {
    let meta: Option<String> = r.get(9)?;
    let meta: Value = meta.and_then(|m| serde_json::from_str(&m).ok()).unwrap_or(Value::Null);
    let mut apps = vec![];
    if let Some(sites) = meta.get("appSites").and_then(crate::types::app_sites_from_value) {
        apps.extend(sites.primary);
        apps.extend(sites.secondary);
    }
    let distraction_count = meta.get("distractions").and_then(Value::as_array).map(Vec::len).unwrap_or(0);
    Ok(Activity {
        record_id: r.get(0)?,
        day: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
        start: r.get(2)?,
        end: r.get(3)?,
        title: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
        summary: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
        detailed_summary: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
        category: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
        subcategory: r.get::<_, Option<String>>(8)?.unwrap_or_default(),
        apps,
        distraction_count,
    })
}

fn collect(conn: &Connection, sql: &str, params: impl rusqlite::Params) -> QResult<Vec<Activity>> {
    let mut st = conn.prepare(sql)?;
    let rows = st.query_map(params, activity_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Cards starting inside one day's window.
pub fn fetch_day(conn: &Connection, w: &DayWindow) -> QResult<Vec<Activity>> {
    collect(
        conn,
        &format!("SELECT {CARD_COLUMNS} FROM timeline_cards WHERE start_ts >= ?1 AND start_ts < ?2 AND is_deleted = 0 ORDER BY start_ts ASC"),
        [w.start, w.end],
    )
}

/// Cards overlapping [from, to).
pub fn fetch_range(conn: &Connection, from: i64, to: i64) -> QResult<Vec<Activity>> {
    collect(
        conn,
        &format!("SELECT {CARD_COLUMNS} FROM timeline_cards WHERE start_ts < ?1 AND end_ts > ?2 AND is_deleted = 0 ORDER BY start_ts ASC"),
        [to, from],
    )
}

pub fn fetch_activity(conn: &Connection, id: i64) -> QResult<Option<Activity>> {
    Ok(conn
        .query_row(&format!("SELECT {CARD_COLUMNS} FROM timeline_cards WHERE id = ?1 AND is_deleted = 0"), [id], activity_from_row)
        .optional()?)
}

fn like_pattern(text: &str) -> String {
    let escaped = text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
    format!("%{escaped}%")
}

/// Cards whose title or summary contains `text` (case-insensitive for ASCII), newest first.
pub fn search(conn: &Connection, text: &str, range: Option<(i64, i64)>, limit: usize) -> QResult<Vec<Activity>> {
    let pattern = like_pattern(text.trim());
    let (from, to) = range.unwrap_or((i64::MIN, i64::MAX));
    collect(
        conn,
        &format!(
            "SELECT {CARD_COLUMNS} FROM timeline_cards
             WHERE (title LIKE ?1 ESCAPE '\\' OR summary LIKE ?1 ESCAPE '\\') AND is_deleted = 0
               AND start_ts < ?2 AND end_ts > ?3
             ORDER BY start_ts DESC LIMIT ?4"
        ),
        rusqlite::params![pattern, to, from, limit as i64],
    )
}

/// Minutes per category, System left out, largest first.
pub fn category_totals(activities: &[Activity]) -> Vec<(String, i64)> {
    let mut totals: BTreeMap<String, i64> = BTreeMap::new();
    for a in activities.iter().filter(|a| a.category != SYSTEM_CATEGORY) {
        *totals.entry(a.category.clone()).or_default() += a.duration_minutes();
    }
    let mut v: Vec<(String, i64)> = totals.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    v
}

pub fn categories_json(totals: &[(String, i64)]) -> (i64, Value) {
    let tracked: i64 = totals.iter().map(|(_, m)| m).sum();
    let list = totals
        .iter()
        // Shares rounded to 4 decimals keep the JSON short and stable.
        .map(|(name, m)| json!({"name": name, "minutes": m, "share": if tracked > 0 { (*m as f64 / tracked as f64 * 10_000.0).round() / 10_000.0 } else { 0.0 }}))
        .collect();
    (tracked, Value::Array(list))
}

/// One activity as JSON (Dayflow's json(for:detailed:), plus the Daytrace day).
pub fn activity_json(a: &Activity, detailed: bool) -> Value {
    let mut o = Map::new();
    o.insert("record_id".into(), json!(a.record_id));
    o.insert("date".into(), json!(a.day));
    o.insert("start".into(), json!(iso(a.start)));
    o.insert("end".into(), json!(iso(a.end)));
    o.insert("duration_minutes".into(), json!(a.duration_minutes()));
    o.insert("title".into(), json!(a.title));
    o.insert("summary".into(), json!(a.summary));
    o.insert("category".into(), json!(a.category));
    if !a.subcategory.is_empty() {
        o.insert("subcategory".into(), json!(a.subcategory));
    }
    if !a.apps.is_empty() {
        o.insert("apps".into(), json!(a.apps));
    }
    if a.distraction_count > 0 {
        o.insert("distraction_count".into(), json!(a.distraction_count));
    }
    if detailed && !a.detailed_summary.is_empty() {
        o.insert("detailed_summary".into(), json!(a.detailed_summary));
    }
    Value::Object(o)
}

pub fn timeline_envelope(activities: &[Activity], day: &str, detailed: bool, hint: &str) -> Value {
    json!({
        "date": day,
        "time_zone": time_zone(),
        "day_boundary_hour": DAY_START_HOUR,
        "cards": activities.iter().map(|a| activity_json(a, detailed)).collect::<Vec<_>>(),
        "detail_available": !detailed,
        "hint": if detailed { "" } else { hint },
    })
}

/// Totals for a range too long to list (Dayflow's renderRangeAggregate, JSON form).
pub fn range_aggregate_json(activities: &[Activity], from: &str, to: &str, days: i64) -> Value {
    let (tracked, cats) = categories_json(&category_totals(activities));
    json!({
        "range": {"from": from, "to": to, "days": days},
        "granularity": "aggregate",
        "cards_omitted": activities.len(),
        "reason": format!("Range exceeds {MAX_RANGE_DAYS_FOR_ACTIVITIES} days. Returning totals instead of individual activities."),
        "tracked_minutes": tracked,
        "categories": cats,
        "hint": format!("Request {MAX_RANGE_DAYS_FOR_ACTIVITIES} days or fewer to get individual activities."),
    })
}

// ---- daily recap (standup) ----

#[derive(Debug, Clone, PartialEq)]
pub struct Standup {
    pub day: String,
    pub source_day: String,
    pub highlights: Vec<String>,
    pub tasks: Vec<String>,
    pub blockers: Vec<String>,
}

fn table_exists(conn: &Connection, name: &str) -> QResult<bool> {
    let n: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1", [name], |r| r.get(0))?;
    Ok(n > 0)
}

/// The recap written for `day` (Daytrace's daily_recaps, keyed by the day the standup is for).
/// Databases from before the Daily view have no such table; that reads as "no recap".
pub fn fetch_standup(conn: &Connection, day: &str) -> QResult<Option<Standup>> {
    if !table_exists(conn, "daily_recaps")? {
        return Ok(None);
    }
    let list = |s: Option<String>| -> Vec<String> { s.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default() };
    Ok(conn
        .query_row(
            "SELECT target_day, source_day, highlights, tasks, blockers FROM daily_recaps WHERE target_day = ?1",
            [day],
            |r| {
                Ok(Standup {
                    day: r.get(0)?,
                    source_day: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    highlights: list(r.get(2)?),
                    tasks: list(r.get(3)?),
                    blockers: list(r.get(4)?),
                })
            },
        )
        .optional()?)
}

pub fn standup_json(s: &Standup) -> Value {
    json!({
        "date": s.day,
        "source_date": s.source_day,
        "highlights": s.highlights,
        "tasks": s.tasks,
        "blockers": s.blockers.join("\n"),
    })
}

// ---- observations ----

#[derive(Debug, Clone, PartialEq)]
pub struct ObservationRow {
    pub start: i64,
    pub end: i64,
    pub text: String,
}

/// Screen observations overlapping a card's time range, in order.
pub fn fetch_observations(conn: &Connection, a: &Activity) -> QResult<Vec<ObservationRow>> {
    let mut st = conn.prepare("SELECT start_ts, end_ts, observation FROM observations WHERE start_ts < ?1 AND end_ts > ?2 ORDER BY start_ts ASC")?;
    let rows = st
        .query_map([a.end, a.start], |r| Ok(ObservationRow { start: r.get(0)?, end: r.get(1)?, text: r.get(2)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn observations_json(a: &Activity, rows: &[ObservationRow]) -> Value {
    let mut v = json!({
        "record_id": a.record_id,
        "title": a.title,
        "start": iso(a.start),
        "end": iso(a.end),
        "observations": rows.iter().map(|o| json!({"start": iso(o.start), "end": iso(o.end), "text": o.text})).collect::<Vec<_>>(),
    });
    if rows.is_empty() {
        v["hint"] = json!("No observations for this activity. Daytrace deletes observations together with screenshots after the retention period; the activity itself is kept.");
    }
    v
}

// ---- days with data ----

#[derive(Debug, Clone, PartialEq)]
pub struct DayRow {
    pub day: String,
    pub activities: i64,
    pub tracked_minutes: i64,
}

/// Days that have activities (System cards left out), newest first.
pub fn list_days(conn: &Connection, limit: usize) -> QResult<Vec<DayRow>> {
    let mut st = conn.prepare(
        "SELECT day, COUNT(*), COALESCE(SUM(MAX(end_ts - start_ts, 0) / 60), 0) FROM timeline_cards
         WHERE is_deleted = 0 AND category != 'System' GROUP BY day ORDER BY day DESC LIMIT ?1",
    )?;
    let rows = st
        .query_map([limit as i64], |r| Ok(DayRow { day: r.get(0)?, activities: r.get(1)?, tracked_minutes: r.get(2)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn days_json(rows: &[DayRow]) -> Value {
    json!({
        "days": rows.iter().map(|d| json!({"date": d.day, "activities": d.activities, "tracked_minutes": d.tracked_minutes})).collect::<Vec<_>>(),
        "day_boundary_hour": DAY_START_HOUR,
    })
}

// ---- status ----

#[derive(Debug, Clone, PartialEq)]
pub struct Status {
    pub today: String,
    pub last_capture_at: Option<i64>,
    pub pending_batches: i64,
    pub failed_batches: i64,
}

pub fn fetch_status(conn: &Connection) -> QResult<Status> {
    let last: Option<i64> = conn.query_row("SELECT MAX(captured_at) FROM screenshots WHERE is_deleted = 0", [], |r| r.get(0))?;
    let (pending, failed): (Option<i64>, Option<i64>) = conn.query_row(
        "SELECT SUM(CASE WHEN status IN ('pending', 'processing') THEN 1 ELSE 0 END),
                SUM(CASE WHEN status LIKE 'failed%' THEN 1 ELSE 0 END) FROM analysis_batches",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(Status { today: today_window().day, last_capture_at: last, pending_batches: pending.unwrap_or(0), failed_batches: failed.unwrap_or(0) })
}

pub fn status_json(s: &Status) -> Value {
    let mut v = json!({
        "today": s.today,
        "time_zone": time_zone(),
        "day_boundary_hour": DAY_START_HOUR,
        "pending_batches": s.pending_batches,
        "failed_batches": s.failed_batches,
        "edits_enabled": false,
    });
    if let Some(t) = s.last_capture_at {
        v["last_capture_at"] = json!(iso(t));
    }
    v
}

pub fn categories_list_json(cats: &[Category]) -> Value {
    json!({
        "categories": cats.iter().map(|c| json!({
            "name": c.name, "color_hex": c.color, "description": c.description,
            "is_system": c.is_idle, "is_idle": c.is_idle,
        })).collect::<Vec<_>>(),
    })
}

/// Adds `schema_version` to a top-level object.
pub fn versioned(mut v: Value) -> Value {
    if let Some(o) = v.as_object_mut() {
        o.insert("schema_version".into(), json!(SCHEMA_VERSION));
    }
    v
}

#[cfg(test)]
pub(crate) mod fixture {
    //! A temporary Daytrace database with a few cards, written through the app's own Db.
    use super::*;
    use crate::db::{Db, ResolvedCard};
    use crate::types::{ActivityCard, AppSites, Distraction};

    pub struct Fixture {
        pub dir: tempfile::TempDir,
        pub source: Source,
        pub day: String,
        pub day_start: i64,
        pub ids: Vec<i64>,
    }

    /// Timestamp `h:m` hours after 4 AM on the fixture day.
    pub fn at(day_start: i64, h: i64, m: i64) -> i64 {
        day_start + (h * 60 + m) * 60
    }

    fn card(title: &str, cat: &str, s: i64, e: i64, summary: &str, detailed: &str) -> ResolvedCard {
        ResolvedCard {
            start_ts: s,
            end_ts: e,
            card: ActivityCard {
                start_time: crate::timeutil::fmt_clock(s),
                end_time: crate::timeutil::fmt_clock(e),
                category: cat.into(),
                subcategory: String::new(),
                title: title.into(),
                summary: summary.into(),
                detailed_summary: detailed.into(),
                distractions: None,
                app_sites: None,
            },
            idle_meta: None,
        }
    }

    /// Day 2026-03-11: Work 5:00-6:00 (after 4 AM), Personal 6:00-6:30, Work 6:30-7:15 with
    /// apps and a distraction, a System error card 7:15-7:30; one Work card on 2026-03-10.
    pub fn build() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join(DB_FILE)).unwrap();
        let day = "2026-03-11".to_string();
        let (s, _) = logical_day_bounds(&day).unwrap();
        let (prev, _) = logical_day_bounds("2026-03-10").unwrap();
        // Screenshots: one with an image and a private app row; neither must leak.
        let shot = db.insert_screenshot(at(s, 1, 0), "C:/secret/path/shot.jpg", 1000, 0, Some("code.exe"), Some("main.rs"), None, None).unwrap();
        db.insert_screenshot(at(s, 1, 1), "", 0, 0, Some("KeePass.exe"), None, Some("KeePass.exe"), None).unwrap();
        let shots = db.screenshots_in_range(0, i64::MAX).unwrap();
        assert_eq!(shots[0].id, shot);
        let batch = db.create_batch(&shots).unwrap();
        let mut c3 = card("Fix 100% coverage_report bug", "Work", at(s, 2, 30), at(s, 3, 15), "Debugged the coverage report.", "Long write-up about the coverage report.");
        c3.card.app_sites = Some(AppSites { primary: Some("github.com".into()), secondary: Some("VS Code".into()) });
        c3.card.distractions = Some(vec![Distraction { start_time: "6:40 AM".into(), end_time: "6:45 AM".into(), title: "YouTube".into(), summary: String::new() }]);
        let cards = vec![
            card("Write the parser", "Work", at(s, 1, 0), at(s, 2, 0), "Wrote the CSV parser.", "Detailed parser notes."),
            card("Pay bills", "Personal", at(s, 2, 0), at(s, 2, 30), "Paid the electricity bill.", ""),
            c3,
            card(crate::db::ERROR_TITLE, SYSTEM_CATEGORY, at(s, 3, 15), at(s, 3, 30), "The model returned an unusable response.", ""),
        ];
        db.replace_cards_in_range(at(s, 0, 0), at(s, 4, 0), &cards, batch, false).unwrap();
        db.replace_cards_in_range(at(prev, 10, 0), at(prev, 11, 0), &[card("Plan the week", "Work", at(prev, 10, 0), at(prev, 11, 0), "Planned the week.", "")], batch, false)
            .unwrap();
        db.insert_observations(
            batch,
            &[
                crate::types::Observation { start_ts: at(s, 1, 0), end_ts: at(s, 1, 30), text: "Editing parser.rs in VS Code.".into() },
                crate::types::Observation { start_ts: at(s, 1, 30), end_ts: at(s, 2, 0), text: "Running cargo test.".into() },
                crate::types::Observation { start_ts: at(s, 2, 0), end_ts: at(s, 2, 30), text: "Bank website.".into() },
            ],
            Some("test-model"),
        )
        .unwrap();
        let ids = db.cards_in_range(at(s, 0, 0), at(s, 4, 0), false).unwrap().iter().map(|c| c.id).collect();
        drop(db);
        let source = Source::from_dir(dir.path().to_path_buf());
        Fixture { dir, source, day, day_start: s, ids }
    }

    pub fn add_recap(f: &Fixture) {
        let c = Connection::open(&f.source.db_path).unwrap();
        c.execute(
            "INSERT INTO daily_recaps (target_day, source_day, highlights, tasks, blockers, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, 0, 0)",
            rusqlite::params!["2026-03-12", f.day, r#"["Wrote the CSV parser"]"#, r#"["Ship the parser"]"#, r#"["Waiting on review"]"#],
        )
        .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::*;
    use super::*;

    #[test]
    fn day_and_range_queries_follow_dayflow() {
        let f = build();
        let c = f.source.open().unwrap();
        let w = day_window(&f.day).unwrap();
        let day = fetch_day(&c, &w).unwrap();
        assert_eq!(day.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(), ["Write the parser", "Pay bills", "Fix 100% coverage_report bug", "Processing failed"]);
        assert_eq!(day[2].apps, ["github.com", "VS Code"]);
        assert_eq!(day[2].distraction_count, 1);
        let (a, b) = range_windows("2026-03-10", &f.day).unwrap();
        assert_eq!(days_between(&a, &b), 2);
        let all = fetch_range(&c, a.start, b.end).unwrap();
        assert_eq!(all.len(), 5);
        let totals = category_totals(&all);
        assert_eq!(totals, vec![("Work".to_string(), 60 + 45 + 60), ("Personal".to_string(), 30)]);
        assert!(range_windows(&f.day, "2026-03-10").is_err());
        assert!(day_window("11/03/2026").is_err());
    }

    #[test]
    fn read_only_connection_cannot_write() {
        let f = build();
        let c = f.source.open().unwrap();
        assert!(c.execute("DELETE FROM timeline_cards", []).is_err());
        let missing = Source::from_dir(f.dir.path().join("nope"));
        assert!(matches!(missing.open(), Err(QueryError::NoData(_))));
    }

    #[test]
    fn search_escapes_wildcards_and_respects_range() {
        let f = build();
        let c = f.source.open().unwrap();
        assert_eq!(search(&c, "100%", None, 50).unwrap().len(), 1);
        assert_eq!(search(&c, "coverage_report", None, 50).unwrap().len(), 1);
        assert_eq!(search(&c, "_", None, 50).unwrap().len(), 1, "underscore is literal");
        assert_eq!(search(&c, "PARSER", None, 50).unwrap().len(), 1, "case-insensitive");
        let all = search(&c, "the", None, 50).unwrap();
        assert!(all.len() >= 3);
        assert!(all.windows(2).all(|w| w[0].start >= w[1].start), "newest first");
        let prev = day_window("2026-03-10").unwrap();
        let only_prev = search(&c, "the", Some((prev.start, prev.end)), 50).unwrap();
        assert_eq!(only_prev.iter().map(|a| a.title.as_str()).collect::<Vec<_>>(), ["Plan the week"]);
    }

    #[test]
    fn standup_tolerates_missing_table() {
        let f = build();
        let c = f.source.open().unwrap();
        assert_eq!(fetch_standup(&c, "2026-03-12").unwrap(), None);
        add_recap(&f);
        let c = f.source.open().unwrap();
        let s = fetch_standup(&c, "2026-03-12").unwrap().unwrap();
        assert_eq!(s.highlights, ["Wrote the CSV parser"]);
        assert_eq!(standup_json(&s)["blockers"], "Waiting on review");
        // A database without the insights table.
        let dir = tempfile::tempdir().unwrap();
        let raw = Connection::open(dir.path().join(DB_FILE)).unwrap();
        raw.execute_batch("CREATE TABLE timeline_cards (id INTEGER)").unwrap();
        drop(raw);
        let c = Source::from_dir(dir.path().to_path_buf()).open().unwrap();
        assert_eq!(fetch_standup(&c, "2026-03-12").unwrap(), None);
    }

    #[test]
    fn observations_days_status_and_categories() {
        let f = build();
        let c = f.source.open().unwrap();
        let first = fetch_activity(&c, f.ids[0]).unwrap().unwrap();
        let obs = fetch_observations(&c, &first).unwrap();
        assert_eq!(obs.iter().map(|o| o.text.as_str()).collect::<Vec<_>>(), ["Editing parser.rs in VS Code.", "Running cargo test."]);
        let days = list_days(&c, 30).unwrap();
        assert_eq!(days[0], DayRow { day: f.day.clone(), activities: 3, tracked_minutes: 135 });
        assert_eq!(days[1].day, "2026-03-10");
        let st = fetch_status(&c).unwrap();
        assert_eq!(st.last_capture_at, Some(at(f.day_start, 1, 1)));
        assert_eq!(st.pending_batches, 1);
        // Categories: defaults without settings.json, the user's list with one.
        assert_eq!(f.source.categories().len(), 4);
        std::fs::write(
            f.dir.path().join("settings.json"),
            r##"{"categories":[{"name":"Deep work","color":"#112233","description":"Focus","isIdle":false},{"name":"Away","color":"#999999","isIdle":true}]}"##,
        )
        .unwrap();
        assert_eq!(f.source.categories().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Deep work", "Away"]);
        assert_eq!(f.source.idle_category(), "Away");
    }

    #[test]
    fn source_resolution_order() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path().to_string_lossy().to_string();
        let file = dir.path().join("other.sqlite").to_string_lossy().to_string();
        assert_eq!(Source::resolve(Some(&d), Some("/elsewhere")), Source::from_dir(dir.path().to_path_buf()));
        let s = Source::resolve(Some(&file), None);
        assert_eq!(s.db_path, dir.path().join("other.sqlite"));
        assert_eq!(s.data_dir, dir.path());
        assert_eq!(Source::resolve(None, Some(&d)).db_path, dir.path().join(DB_FILE));
        assert_eq!(Source::resolve(Some("plain.sqlite"), None).data_dir, PathBuf::from("."));
    }

    #[test]
    fn weeks_start_monday_4am() {
        // 2026-03-11 is a Wednesday.
        let (s, e) = week_window(&day_window("2026-03-11").unwrap()).unwrap();
        assert_eq!(logical_day(s), "2026-03-09");
        assert_eq!(logical_day(e), "2026-03-16");
        assert_eq!(local(s).format("%H:%M").to_string(), "04:00");
        let (s2, _) = week_window(&day_window("2026-03-09").unwrap()).unwrap();
        assert_eq!(s, s2);
    }
}
