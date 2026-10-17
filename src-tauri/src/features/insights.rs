//! Insights: the Daily view (day statistics and the standup recap), the Weekly review and the
//! recap Markdown export for reading on a phone. Ported from Dayflow (MIT, (c) 2025 Jerry Liu);
//! each submodule names the Swift files it follows.

pub mod export;
pub mod prompts;
pub mod recap;
pub mod stats;
pub mod weekly;

use crate::config::ProviderKind;
use crate::db::{Db, StoredCard, SYSTEM_CATEGORY};
use crate::llm::client::{CallOpts, LlmClient, Part};
use crate::state::AppState;
use crate::timeutil::{logical_day, logical_day_bounds, now_ts, DAY_START_HOUR};
use anyhow::{anyhow, bail, Result};
use chrono::{Duration as CDuration, Local, NaiveDate, TimeZone};
use recap::RecapEntry;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use stats::{DayStats, NamedSecs};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use weekly::{DayInput, WeekStats};

/// Tables owned by this feature (CREATE TABLE IF NOT EXISTS ...).
pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS daily_recaps (
    target_day TEXT PRIMARY KEY,
    source_day TEXT NOT NULL,
    highlights TEXT NOT NULL DEFAULT '[]',
    tasks TEXT NOT NULL DEFAULT '[]',
    blockers TEXT NOT NULL DEFAULT '[]',
    provider TEXT,
    model TEXT,
    edited INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_daily_recaps_source ON daily_recaps(source_day);
CREATE TABLE IF NOT EXISTS insights_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

/// Dayflow's DailyRecapScheduler checks every 5 minutes.
const CHECK_INTERVAL: Duration = Duration::from_secs(5 * 60);
/// DailyRecapAttemptBudget.maxAttempts: automatic tries per target day.
const MAX_AUTO_ATTEMPTS: u32 = 3;
/// How long the automatic recap waits for the source day's batches to finish analysis.
const MAX_ANALYSIS_WAIT: i64 = 2 * 3600;

type S<'a> = tauri::State<'a, Arc<AppState>>;
type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

// ---- small key-value state ----

fn kv_get(c: &Connection, key: &str) -> Result<Option<String>> {
    Ok(c.query_row("SELECT value FROM insights_state WHERE key = ?", [key], |r| r.get(0)).optional()?)
}

fn kv_set(c: &Connection, key: &str, value: &str) -> Result<()> {
    c.execute("INSERT INTO insights_state (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value])?;
    Ok(())
}

fn kv_del(c: &Connection, key: &str) -> Result<()> {
    c.execute("DELETE FROM insights_state WHERE key = ?", [key])?;
    Ok(())
}

fn auto_error_key(target: &str) -> String {
    format!("auto_error:{target}")
}

// ---- data ----

/// Foreground-program time in [from, to) from screenshot metadata. Rows taken while away or with a
/// private app in front keep their place in the sequence but count for nobody.
fn programs(db: &Db, from: i64, to: i64, default_secs: i64, limit: usize) -> Result<Vec<NamedSecs>> {
    let rows = {
        let c = db.conn();
        let mut st = c.prepare(
            "SELECT captured_at, CASE WHEN skip_reason IS NULL AND hidden_app IS NULL THEN app_name END
             FROM screenshots WHERE captured_at >= ? AND captured_at < ? ORDER BY captured_at",
        )?;
        st.query_map([from, to], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?
    };
    Ok(stats::program_usage(&rows, default_secs, limit))
}

fn bounds(day: &str) -> Result<(i64, i64)> {
    logical_day_bounds(day).ok_or_else(|| anyhow!("Invalid day: {day}"))
}

pub fn day_stats(state: &AppState, day: &str) -> Result<DayStats> {
    let (s, e) = bounds(day)?;
    let settings = state.settings();
    let cards = state.db.cards_in_range(s, e, true)?;
    let mut st = stats::compute_day(day, &cards, &settings.categories, s, e);
    st.programs = programs(&state.db, s, e, settings.capture_interval_secs as i64, 8)?;
    Ok(st)
}

fn day_inputs(db: &Db, days: &[String]) -> Result<Vec<DayInput>> {
    days.iter()
        .map(|d| {
            let (start, end) = bounds(d)?;
            Ok(DayInput { day: d.clone(), start, end, cards: db.cards_in_range(start, end, true)? })
        })
        .collect()
}

pub fn week_stats(state: &AppState, day: &str) -> Result<WeekStats> {
    let days = weekly::week_days(day).ok_or_else(|| anyhow!("Invalid day: {day}"))?;
    let monday = NaiveDate::parse_from_str(&days[0], "%Y-%m-%d")?;
    let prev_days = weekly::week_days(&(monday - CDuration::days(7)).format("%Y-%m-%d").to_string()).unwrap_or_default();
    let settings = state.settings();
    let current = day_inputs(&state.db, &days)?;
    let previous = day_inputs(&state.db, &prev_days)?;
    let mut w = weekly::compute_week(&current, &previous, &settings.categories);
    w.programs = programs(&state.db, current[0].start, current[current.len() - 1].end, settings.capture_interval_secs as i64, 10)?;
    Ok(w)
}

// ---- recap ----

static IN_FLIGHT: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// One generation per target day at a time.
struct InFlight(String);

impl InFlight {
    fn claim(day: &str) -> Option<InFlight> {
        let mut g = IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner());
        if g.iter().any(|d| d == day) {
            return None;
        }
        g.push(day.to_string());
        Some(InFlight(day.to_string()))
    }
}

impl Drop for InFlight {
    fn drop(&mut self) {
        IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner()).retain(|d| *d != self.0);
    }
}

fn is_in_flight(day: &str) -> bool {
    IN_FLIGHT.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|d| d == day)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecapView {
    /// The day the standup is for.
    pub target_day: String,
    pub entry: Option<RecapEntry>,
    /// The entry's source day, or the day a new recap would be made from.
    pub source_day: Option<String>,
    pub generating: bool,
    /// The last automatic generation error for this day.
    pub auto_error: Option<String>,
    /// The export file of the source day, when it exists.
    pub export_path: Option<String>,
}

fn existing_export(state: &AppState, source_day: &str) -> Option<PathBuf> {
    let dir = state.settings().recap_export_dir;
    if dir.trim().is_empty() {
        return None;
    }
    let p = export::export_path(&dir, source_day);
    p.exists().then_some(p)
}

pub fn recap_view(state: &AppState, target: &str) -> Result<RecapView> {
    bounds(target)?;
    let settings = state.settings();
    let (entry, source_day, auto_error) = {
        let c = state.db.conn();
        let entry = recap::load(&c, target)?;
        let source = match &entry {
            Some(e) => Some(e.source_day.clone()),
            None => recap::source_for(&c, target, &settings.categories, recap::MIN_MINUTES_MANUAL)?,
        };
        (entry, source, kv_get(&c, &auto_error_key(target))?)
    };
    let export_path = source_day.as_deref().and_then(|s| existing_export(state, s)).map(|p| p.to_string_lossy().to_string());
    Ok(RecapView { target_day: target.to_string(), generating: is_in_flight(target), entry, source_day, auto_error, export_path })
}

/// Generates (or regenerates) the recap for `target`. `background` is the scheduler: it may use
/// the Batch API and needs 180 captured minutes on the source day; the Daily view needs 120 and
/// keeps the source day of an existing recap.
pub async fn generate(state: &AppState, target: &str, background: bool) -> Result<RecapEntry> {
    let _guard = InFlight::claim(target).ok_or_else(|| anyhow!("A recap for this day is already being generated."))?;
    let settings = state.settings();
    let source = {
        let c = state.db.conn();
        match recap::load(&c, target)? {
            Some(e) if !background => Some(e.source_day),
            _ => recap::source_for(&c, target, &settings.categories, if background { recap::MIN_MINUTES_AUTO } else { recap::MIN_MINUTES_MANUAL })?,
        }
    };
    let source = source.ok_or_else(|| anyhow!(recap::INSUFFICIENT_HISTORY))?;
    let cards: Vec<StoredCard> = state.db.cards_for_day(&source)?.into_iter().filter(|c| c.category != SYSTEM_CATEGORY).collect();
    if cards.is_empty() {
        bail!("No timeline cards were found for {source}.");
    }
    let api_key = crate::config::stored_key(settings.provider, &settings.base_url);
    if settings.provider == ProviderKind::Anthropic && api_key.is_none() {
        bail!("Add an Anthropic API key in Settings to generate the recap.");
    }
    let prompt = recap::build_prompt(&source, &cards, recap::prompt_style(settings.provider, &settings.base_url), &settings.output_language);
    let mut client = LlmClient::new(settings.provider, &settings.base_url, &settings.model, api_key, Some(state.db.clone()))?;
    // Only the scheduled recap may wait for the Batch API; a click needs an answer now.
    client.use_batch = background && settings.use_batch_api;
    client.reasoning = settings.reasoning;
    let system = client.is_local().then(|| crate::llm::prompts::SYSTEM_TEXT_LOCAL.to_string());
    let raw = client
        .complete(vec![Part::Text(prompt)], CallOpts { op: "daily_recap", batch_id: None, max_tokens: recap::MAX_OUTPUT_TOKENS, system, attempts: 3 })
        .await?;
    let parsed = recap::parse_response(&raw)?;
    let mut entry = recap::entry_from(parsed, target, &source, recap::provider_label(settings.provider), &settings.model, now_ts())?;
    {
        let c = state.db.conn();
        if let Some(old) = recap::load(&c, target)? {
            entry.created_at = old.created_at;
        }
        recap::save(&c, &entry)?;
        kv_del(&c, &auto_error_key(target))?;
    }
    refresh_export(state, &entry.source_day);
    Ok(entry)
}

/// Saves the user's edits; creates the recap when there is none yet.
pub fn save_edits(state: &AppState, target: &str, highlights: &[String], tasks: &[String], blockers: &[String]) -> Result<RecapEntry> {
    let (s, _) = bounds(target)?;
    let settings = state.settings();
    let entry = {
        let c = state.db.conn();
        let now = now_ts();
        let mut e = match recap::load(&c, target)? {
            Some(e) => e,
            None => {
                let source = recap::source_for(&c, target, &settings.categories, recap::MIN_MINUTES_MANUAL)?
                    .unwrap_or_else(|| logical_day(s - 12 * 3600));
                RecapEntry {
                    target_day: target.into(),
                    source_day: source,
                    highlights: vec![],
                    tasks: vec![],
                    blockers: vec![],
                    provider: String::new(),
                    model: String::new(),
                    edited: true,
                    created_at: now,
                    updated_at: now,
                }
            }
        };
        e.highlights = recap::clean_items(highlights);
        e.tasks = recap::clean_items(tasks);
        e.blockers = recap::clean_items(blockers);
        e.edited = true;
        e.updated_at = now;
        recap::save(&c, &e)?;
        e
    };
    refresh_export(state, &entry.source_day);
    Ok(entry)
}

/// Writes the recap file for `day` (the day the file describes) to `path`, or to the export folder.
pub fn export_day(state: &AppState, day: &str, path: Option<PathBuf>, note: Option<&str>) -> Result<PathBuf> {
    let st = day_stats(state, day)?;
    let cards = state.db.cards_for_day(day)?;
    let entry = recap::load_by_source(&state.db.conn(), day)?;
    let md = export::recap_markdown(day, &st, entry.as_ref(), &cards, note);
    let path = match path {
        Some(p) => p,
        None => {
            let dir = state.settings().recap_export_dir;
            if dir.trim().is_empty() {
                bail!("Choose a recap folder in Settings first.");
            }
            export::export_path(&dir, day)
        }
    };
    export::write_file(&path, &md)?;
    Ok(path)
}

/// Keeps an already exported file in step with a regenerated or edited recap.
fn refresh_export(state: &AppState, source_day: &str) {
    if let Some(p) = existing_export(state, source_day) {
        if let Err(e) = export_day(state, source_day, Some(p), None) {
            log::warn!("could not update the recap file for {source_day}: {e:#}");
        }
    }
}

// ---- automatic recap ----

/// "HH:MM" (24-hour clock).
pub fn parse_hhmm(s: &str) -> Option<(u32, u32)> {
    let (h, m) = s.trim().split_once(':')?;
    let (h, m) = (h.trim().parse::<u32>().ok()?, m.trim().parse::<u32>().ok()?);
    (h < 24 && m < 60).then_some((h, m))
}

/// The first moment of `time` inside the logical day `target` (times before 4 AM fall on the next
/// calendar date). Invalid times count as 08:00.
pub fn recap_moment(target: &str, time: &str) -> Option<i64> {
    let (h, m) = parse_hhmm(time).unwrap_or((8, 0));
    let mut date = NaiveDate::parse_from_str(target, "%Y-%m-%d").ok()?;
    if h < DAY_START_HOUR {
        date += CDuration::days(1);
    }
    let naive = date.and_hms_opt(h, m, 0)?;
    Local
        .from_local_datetime(&naive)
        .earliest()
        .or_else(|| Local.from_local_datetime(&(naive + CDuration::hours(1))).earliest())
        .map(|d| d.timestamp())
}

/// DailyRecapAttemptBudget.reserveAttempt: the attempt number, or None after three.
fn reserve_attempt(c: &Connection, target: &str) -> Result<Option<u32>> {
    let count = kv_get(c, "auto_attempts")?
        .and_then(|v| {
            let (d, n) = v.split_once('|')?;
            (d == target).then(|| n.parse::<u32>().ok()).flatten()
        })
        .unwrap_or(0);
    if count >= MAX_AUTO_ATTEMPTS {
        return Ok(None);
    }
    kv_set(c, "auto_attempts", &format!("{target}|{}", count + 1))?;
    Ok(Some(count + 1))
}

/// Batches of the source day that are still waiting for analysis.
fn analysis_pending(state: &AppState, source: &str) -> Result<bool> {
    let (s, e) = bounds(source)?;
    Ok(state.db.batches_with_status(&["pending", "processing"], s - 3600)?.iter().any(|b| b.start_ts < e))
}

/// One scheduler check: after the recap time, make today's recap (from the last active day) if
/// there is none, then write its file to the export folder once.
pub async fn auto_tick(state: &AppState) -> Result<()> {
    let settings = state.settings();
    if !settings.recap_auto {
        return Ok(());
    }
    let now = now_ts();
    let target = logical_day(now);
    let Some(moment) = recap_moment(&target, &settings.recap_time) else { return Ok(()) };
    if now < moment {
        return Ok(());
    }
    let (mut entry, source) = {
        let c = state.db.conn();
        let entry = recap::load(&c, &target)?;
        let source = match &entry {
            Some(e) => Some(e.source_day.clone()),
            None => recap::source_for(&c, &target, &settings.categories, recap::MIN_MINUTES_AUTO)?,
        };
        (entry, source)
    };
    let Some(source) = source else { return Ok(()) };

    let mut note: Option<String> = None;
    if entry.is_none() {
        if now < moment + MAX_ANALYSIS_WAIT && analysis_pending(state, &source)? {
            return Ok(());
        }
        let attempt = reserve_attempt(&state.db.conn(), &target)?;
        match attempt {
            Some(n) => {
                log::info!("generating the daily recap for {target} from {source} (attempt {n})");
                match generate(state, &target, true).await {
                    Ok(e) => entry = Some(e),
                    Err(e) => {
                        let msg = format!("{e:#}");
                        log::warn!("daily recap for {target} failed: {msg}");
                        kv_set(&state.db.conn(), &auto_error_key(&target), &msg)?;
                        if n < MAX_AUTO_ATTEMPTS {
                            return Ok(());
                        }
                        note = Some(format!("The recap could not be generated: {msg}"));
                    }
                }
            }
            None => {
                let last = kv_get(&state.db.conn(), &auto_error_key(&target))?;
                note = Some(format!("The recap could not be generated: {}", last.unwrap_or_else(|| "unknown error".into())));
            }
        }
    }

    if settings.recap_export_dir.trim().is_empty() {
        return Ok(());
    }
    if kv_get(&state.db.conn(), "auto_export_target")?.as_deref() == Some(target.as_str()) {
        return Ok(());
    }
    let day = entry.as_ref().map(|e| e.source_day.clone()).unwrap_or(source);
    let path = export_day(state, &day, None, note.as_deref())?;
    kv_set(&state.db.conn(), "auto_export_target", &target)?;
    log::info!("recap for {day} written to {}", path.display());
    Ok(())
}

/// Starts the daily recap check (every 5 minutes, first after a minute).
pub fn spawn(state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(60)).await;
        loop {
            let s = state.clone();
            match tauri::async_runtime::spawn(async move { auto_tick(&s).await }).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::error!("recap check failed: {e:#}"),
                Err(e) => log::error!("recap check panicked: {e}"),
            }
            tokio::time::sleep(CHECK_INTERVAL).await;
        }
    });
}

// ---- commands ----

#[tauri::command]
pub fn insights_day(state: S, day: String) -> CmdResult<DayStats> {
    day_stats(&state, &day).map_err(err)
}

#[tauri::command]
pub fn insights_week(state: S, day: String) -> CmdResult<WeekStats> {
    week_stats(&state, &day).map_err(err)
}

#[tauri::command]
pub fn insights_recap(state: S, day: String) -> CmdResult<RecapView> {
    recap_view(&state, &day).map_err(err)
}

#[tauri::command]
pub async fn insights_generate_recap(state: S<'_>, day: String) -> CmdResult<RecapView> {
    let st: Arc<AppState> = state.inner().clone();
    generate(&st, &day, false).await.map_err(err)?;
    recap_view(&st, &day).map_err(err)
}

#[tauri::command]
pub fn insights_save_recap(state: S, day: String, highlights: Vec<String>, tasks: Vec<String>, blockers: Vec<String>) -> CmdResult<RecapView> {
    save_edits(&state, &day, &highlights, &tasks, &blockers).map_err(err)?;
    recap_view(&state, &day).map_err(err)
}

/// Writes the recap file for `day` to `path`, or into the export folder when `path` is empty.
/// Returns the path written.
#[tauri::command]
pub fn insights_export_recap(state: S, day: String, path: Option<String>) -> CmdResult<String> {
    let path = path.filter(|p| !p.trim().is_empty()).map(PathBuf::from);
    export_day(&state, &day, path, None).map(|p| p.to_string_lossy().to_string()).map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Paths;
    use crate::db::ResolvedCard;
    use crate::types::ActivityCard;

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let st = AppState::new(Paths::new(dir.path().join("data")).unwrap()).unwrap();
        (dir, st)
    }

    fn add_card(st: &AppState, start: i64, end: i64, category: &str, title: &str) {
        st.db.insert_screenshot(start, "", 0, 0, Some("code"), None, None, None).unwrap();
        let shots = st.db.unbatched_screenshots(0).unwrap();
        let b = st.db.create_batch(&shots).unwrap();
        let card = ActivityCard {
            start_time: crate::timeutil::fmt_clock(start),
            end_time: crate::timeutil::fmt_clock(end),
            category: category.into(),
            subcategory: String::new(),
            title: title.into(),
            summary: format!("{title} summary"),
            detailed_summary: String::new(),
            distractions: None,
            app_sites: None,
        };
        st.db.replace_cards_in_range(start, end, &[ResolvedCard { start_ts: start, end_ts: end, card, idle_meta: None }], b, true).unwrap();
        st.db.set_batch_status(b, "completed", None).unwrap();
    }

    #[test]
    fn hhmm_and_moment() {
        assert_eq!(parse_hhmm("08:00"), Some((8, 0)));
        assert_eq!(parse_hhmm(" 7:5 "), Some((7, 5)));
        assert_eq!(parse_hhmm("24:00"), None);
        assert_eq!(parse_hhmm("eight"), None);
        let eight = Local.with_ymd_and_hms(2026, 10, 17, 8, 0, 0).unwrap().timestamp();
        assert_eq!(recap_moment("2026-10-17", "08:00"), Some(eight));
        assert_eq!(recap_moment("2026-10-17", "garbage"), Some(eight));
        let two_am_next = Local.with_ymd_and_hms(2026, 10, 18, 2, 0, 0).unwrap().timestamp();
        assert_eq!(recap_moment("2026-10-17", "02:00"), Some(two_am_next));
    }

    #[test]
    fn attempt_budget() {
        let (_d, st) = state();
        let c = st.db.conn();
        assert_eq!(reserve_attempt(&c, "2026-10-17").unwrap(), Some(1));
        assert_eq!(reserve_attempt(&c, "2026-10-17").unwrap(), Some(2));
        assert_eq!(reserve_attempt(&c, "2026-10-17").unwrap(), Some(3));
        assert_eq!(reserve_attempt(&c, "2026-10-17").unwrap(), None);
        assert_eq!(reserve_attempt(&c, "2026-10-18").unwrap(), Some(1), "a new day starts over");
    }

    #[test]
    fn in_flight_guard() {
        let g = InFlight::claim("2026-01-01").unwrap();
        assert!(is_in_flight("2026-01-01"));
        assert!(InFlight::claim("2026-01-01").is_none());
        drop(g);
        assert!(!is_in_flight("2026-01-01"));
    }

    #[test]
    fn edits_create_and_update_a_recap() {
        let (_d, st) = state();
        let v = recap_view(&st, "2026-10-17").unwrap();
        assert!(v.entry.is_none() && v.source_day.is_none());
        let e = save_edits(&st, "2026-10-17", &["- Wrote the spec".into()], &[], &["Waiting on Lisa".into()]).unwrap();
        assert_eq!(e.source_day, "2026-10-16");
        assert_eq!(e.highlights, vec!["Wrote the spec"]);
        let v = recap_view(&st, "2026-10-17").unwrap();
        assert_eq!(v.entry.unwrap().blockers, vec!["Waiting on Lisa"]);
    }

    #[tokio::test]
    async fn auto_tick_exports_an_existing_recap_once() {
        let (dir, st) = state();
        let out = dir.path().join("phone");
        let today = logical_day(now_ts());
        let (ts, _) = logical_day_bounds(&today).unwrap();
        let yesterday = logical_day(ts - 3600);
        let (ys, _) = logical_day_bounds(&yesterday).unwrap();
        add_card(&st, ys + 5 * 3600, ys + 9 * 3600, "Work", "Thesis chapter");
        {
            let mut s = st.settings.write().unwrap();
            s.recap_auto = true;
            s.recap_time = "04:00".into();
            s.recap_export_dir = out.to_string_lossy().to_string();
        }
        let entry = RecapEntry {
            target_day: today.clone(),
            source_day: yesterday.clone(),
            highlights: vec!["Finished chapter two.".into()],
            tasks: vec![],
            blockers: vec![],
            provider: "anthropic".into(),
            model: "m".into(),
            edited: false,
            created_at: 1,
            updated_at: 1,
        };
        recap::save(&st.db.conn(), &entry).unwrap();
        auto_tick(&st).await.unwrap();
        let file = export::export_path(&out.to_string_lossy(), &yesterday);
        let md = std::fs::read_to_string(&file).unwrap();
        assert!(md.contains("- Finished chapter two."));
        assert!(md.contains("**Thesis chapter**"));
        assert!(md.contains("- Time captured: 4 h"));
        // Once per day: a second check leaves a deleted file alone.
        std::fs::remove_file(&file).unwrap();
        auto_tick(&st).await.unwrap();
        assert!(!file.exists());
        // The view reports no export file now; a manual export writes it again.
        assert!(recap_view(&st, &today).unwrap().export_path.is_none());
        let p = export_day(&st, &yesterday, None, None).unwrap();
        assert_eq!(p, file);
        assert!(recap_view(&st, &today).unwrap().export_path.is_some());
        // Editing the recap rewrites the exported file.
        save_edits(&st, &today, &["Finished chapter three.".into()], &[], &[]).unwrap();
        assert!(std::fs::read_to_string(&file).unwrap().contains("- Finished chapter three."));
    }

    #[tokio::test]
    async fn auto_tick_does_nothing_without_activity_or_when_off() {
        let (dir, st) = state();
        let out = dir.path().join("phone");
        {
            let mut s = st.settings.write().unwrap();
            s.recap_auto = true;
            s.recap_time = "04:00".into();
            s.recap_export_dir = out.to_string_lossy().to_string();
        }
        auto_tick(&st).await.unwrap();
        assert!(!out.exists(), "no source day, no file");
        st.settings.write().unwrap().recap_auto = false;
        auto_tick(&st).await.unwrap();
        assert!(!out.exists());
    }

    #[test]
    fn day_and_week_commands_read_the_database() {
        let (_d, st) = state();
        let (s, _) = logical_day_bounds("2026-10-14").unwrap();
        add_card(&st, s + 5 * 3600, s + 6 * 3600, "Work", "Spec");
        st.db.insert_screenshot(s + 5 * 3600 + 10, "", 0, 0, Some("WINWORD"), None, None, None).unwrap();
        st.db.insert_screenshot(s + 5 * 3600 + 20, "", 0, 0, Some("KeePass"), None, Some("KeePass"), None).unwrap();
        let d = day_stats(&st, "2026-10-14").unwrap();
        assert_eq!(d.captured_secs, 3600);
        assert_eq!(d.totals[0].name, "Work");
        let names: Vec<&str> = d.programs.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"WINWORD") && !names.contains(&"KeePass"), "{names:?}");
        let w = week_stats(&st, "2026-10-14").unwrap();
        assert_eq!(w.week_start, "2026-10-12");
        assert_eq!(w.captured_secs, 3600);
        assert!(day_stats(&st, "not a day").is_err());
    }
}
