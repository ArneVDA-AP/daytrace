//! Day goals. Dayflow retired its Journal; what it has now are day goals, ported here from
//! Dayflow (MIT, (c) 2025 Jerry Liu):
//! - Models/DayGoalPlan.swift and Core/Recording/StorageManager+DayGoals.swift: a focus target and a
//!   distraction limit per day (defaults 4 h 30 min and 2 h), "Skip today", and plans carried
//!   forward from the latest earlier day.
//! - Views/Components/DaySummaryStats.swift: progress is the de-overlapped time of the day's
//!   non-System cards in focus categories and in distraction categories; setup shows yesterday's
//!   minutes and last week's average.
//! - Views/Components/DayGoalFlowView.swift, DayGoalReviewComponents.swift and
//!   MainView/Lifecycle.requestDailyGoalPromptIfNeeded: the morning flow (yesterday's review, then
//!   today's goals), asked only after 3 days with activity and not after 5 skips in a row, and the
//!   prompt that opens it by itself once a day ("Show daily goal popups"; `take_prompt`).
//!
//! Dayflow lets each plan pick its focus and distraction categories. Daytrace uses each
//! category's "Counts as" choice in Settings > Categories, the same numbers as the Daily view
//! (`insights::stats::focus_split`: focus minus the distractions noted inside focus cards,
//! distraction categories plus those noted distractions), so Goals, Daily, Weekly and Your day so
//! far agree.
//!
//! Daytrace addition: a short checklist of things to get done that day, each with the cards that
//! mention it (keyword match on the card text, no model call).

use crate::db::{Db, StoredCard};
use crate::features::insights::stats::{self, Kind};
use crate::state::AppState;
use crate::timeutil::{logical_day, logical_day_bounds, now_ts};
use crate::types::Category;
use anyhow::{anyhow, bail, Result};
use chrono::{Duration, NaiveDate};
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::collections::BTreeSet;
use std::sync::Arc;

/// Tables owned by this feature. `day_goals` has Dayflow's columns; Dayflow's per-plan
/// `day_goal_categories` is not used (see the module comment).
pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS day_goals (
    day TEXT PRIMARY KEY,
    focus_target_minutes INTEGER NOT NULL,
    distraction_limit_minutes INTEGER NOT NULL,
    is_skipped INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS day_goal_items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    day TEXT NOT NULL,
    text TEXT NOT NULL,
    done INTEGER NOT NULL DEFAULT 0,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_day_goal_items_day ON day_goal_items(day, sort_order);
CREATE TABLE IF NOT EXISTS journal_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

/// DayGoalPlan.defaultPlan.
pub const DEFAULT_FOCUS_MINUTES: i64 = 270;
pub const DEFAULT_DISTRACTION_MINUTES: i64 = 120;
/// DayGoalDurationPicker: 0 to 12 hours.
pub const MAX_MINUTES: i64 = 12 * 60;
/// FeatureAccessRequirements.dayGoalRequiredActiveDays / dayGoalMaxConsecutiveSkips.
pub const REQUIRED_ACTIVE_DAYS: i64 = 3;
pub const MAX_CONSECUTIVE_SKIPS: i64 = 5;
const MAX_ITEMS: usize = 30;
const MAX_ITEM_CHARS: usize = 200;
const EVIDENCE_CARDS: usize = 5;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub day: String,
    pub focus_target_minutes: i64,
    pub distraction_limit_minutes: i64,
    pub is_skipped: bool,
    /// The day has its own saved plan (confirmed or skipped).
    pub saved: bool,
    /// When not saved: the earlier day the targets were carried forward from.
    pub carried_from: Option<String>,
}

fn row_plan(r: &rusqlite::Row) -> rusqlite::Result<Plan> {
    Ok(Plan {
        day: r.get(0)?,
        focus_target_minutes: r.get(1)?,
        distraction_limit_minutes: r.get(2)?,
        is_skipped: r.get::<_, i64>(3)? != 0,
        saved: true,
        carried_from: None,
    })
}

const PLAN_COLS: &str = "day, focus_target_minutes, distraction_limit_minutes, is_skipped";

fn explicit_plan(c: &Connection, day: &str) -> Result<Option<Plan>> {
    Ok(c.query_row(&format!("SELECT {PLAN_COLS} FROM day_goals WHERE day = ?"), [day], row_plan).optional()?)
}

/// The day's plan, else the latest earlier plan carried forward (not skipped), else the default
/// (Dayflow carriedForwardGoalPlan).
pub fn effective_plan(db: &Db, day: &str) -> Result<Plan> {
    let c = db.conn();
    if let Some(p) = explicit_plan(&c, day)? {
        return Ok(p);
    }
    let prev = c
        .query_row(&format!("SELECT {PLAN_COLS} FROM day_goals WHERE day < ? ORDER BY day DESC LIMIT 1"), [day], row_plan)
        .optional()?;
    Ok(match prev {
        Some(p) => Plan { day: day.into(), is_skipped: false, saved: false, carried_from: Some(p.day), ..p },
        None => Plan {
            day: day.into(),
            focus_target_minutes: DEFAULT_FOCUS_MINUTES,
            distraction_limit_minutes: DEFAULT_DISTRACTION_MINUTES,
            is_skipped: false,
            saved: false,
            carried_from: None,
        },
    })
}

/// Dayflow saveDayGoalPlan (upsert; created_at kept).
pub fn save_plan(db: &Db, day: &str, focus_minutes: i64, distraction_minutes: i64, skipped: bool) -> Result<()> {
    check_day(day)?;
    let now = now_ts();
    db.conn().execute(
        "INSERT INTO day_goals (day, focus_target_minutes, distraction_limit_minutes, is_skipped, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5)
         ON CONFLICT(day) DO UPDATE SET focus_target_minutes = excluded.focus_target_minutes,
           distraction_limit_minutes = excluded.distraction_limit_minutes, is_skipped = excluded.is_skipped,
           updated_at = excluded.updated_at",
        params![day, focus_minutes.clamp(0, MAX_MINUTES), distraction_minutes.clamp(0, MAX_MINUTES), skipped as i64, now],
    )?;
    Ok(())
}

fn check_day(day: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(day, "%Y-%m-%d").map_err(|_| anyhow!("Invalid day: {day}"))
}

fn shift(day: &str, delta: i64) -> Result<String> {
    Ok((check_day(day)? + Duration::days(delta)).format("%Y-%m-%d").to_string())
}

// ---- progress ----

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategorySecs {
    pub name: String,
    pub color: String,
    pub secs: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    /// Time in focus categories, without the distractions noted inside those cards.
    pub focus_secs: i64,
    /// Time in distraction categories plus the distractions noted inside other cards.
    pub distracted_secs: i64,
    /// Focus categories in settings order, then other focus categories found on cards.
    pub focus_categories: Vec<CategorySecs>,
    /// Names of the categories that count as distraction.
    pub distraction_categories: Vec<String>,
}

/// Focus and distraction time of a day over de-overlapped non-System cards (Dayflow
/// DaySummaryStats.computeTotalFocusTime / computeTotalDistractedTime), with the noted
/// distractions moved from focus to distraction like the Daily view.
pub fn progress(cards: &[StoredCard], cats: &[Category], day_start: i64, day_end: i64) -> Progress {
    let segs = stats::segments(cards, cats, day_start, day_end);
    let split = stats::focus_split(&stats::remove_overlaps(&segs));
    let mut p = Progress { focus_secs: split.focus, distracted_secs: split.distracted, ..Progress::default() };
    for c in cats {
        let info = stats::cat_info(&c.name, cats);
        match info.kind {
            Kind::Focus => p.focus_categories.push(CategorySecs { name: info.name, color: info.color, secs: 0 }),
            Kind::Distraction => p.distraction_categories.push(info.name),
            _ => {}
        }
    }
    for (key, name, color, secs) in split.focus_by_category {
        match p.focus_categories.iter_mut().find(|c| c.name.trim().to_lowercase() == key) {
            Some(c) => c.secs += secs,
            None => p.focus_categories.push(CategorySecs { name, color, secs }),
        }
    }
    p
}

fn day_progress(db: &Db, day: &str, cats: &[Category]) -> Result<Progress> {
    let (from, to) = logical_day_bounds(day).ok_or_else(|| anyhow!("Invalid day: {day}"))?;
    Ok(progress(&db.cards_for_day(day)?, cats, from, to))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Review {
    pub plan: Plan,
    pub progress: Progress,
    /// Focus time reached the target ("Nailed it").
    pub focus_met: bool,
    /// Distracted time stayed within the limit.
    pub distraction_met: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reference {
    pub yesterday_focus_minutes: i64,
    pub last_week_focus_minutes: i64,
    pub yesterday_distraction_minutes: i64,
    pub last_week_distraction_minutes: i64,
}

/// Yesterday's minutes and the average of the 7 days before `day` (Dayflow
/// makeGoalSetupReferenceStats divides by 7 whether or not each day has data).
fn reference(db: &Db, day: &str, cats: &[Category]) -> Result<Reference> {
    let mut r = Reference::default();
    let (mut focus, mut distraction) = (0i64, 0i64);
    for offset in 1..=7 {
        let p = day_progress(db, &shift(day, -offset)?, cats)?;
        if offset == 1 {
            r.yesterday_focus_minutes = p.focus_secs / 60;
            r.yesterday_distraction_minutes = p.distracted_secs / 60;
        }
        focus += p.focus_secs;
        distraction += p.distracted_secs;
    }
    r.last_week_focus_minutes = focus / 7 / 60;
    r.last_week_distraction_minutes = distraction / 7 / 60;
    Ok(r)
}

/// Whether the morning flow should ask for today's goals (Lifecycle.requestDailyGoalPromptIfNeeded).
pub fn should_prompt(db: &Db, day: &str, today: &str) -> Result<bool> {
    if day != today {
        return Ok(false);
    }
    let c = db.conn();
    if explicit_plan(&c, day)?.is_some() {
        return Ok(false);
    }
    let active: i64 = c.query_row("SELECT COUNT(DISTINCT day) FROM timeline_cards WHERE is_deleted = 0 AND day != ?", [day], |r| r.get(0))?;
    if active < REQUIRED_ACTIVE_DAYS {
        return Ok(false);
    }
    let flags: Vec<i64> = {
        let mut st = c.prepare("SELECT is_skipped FROM day_goals WHERE day < ? ORDER BY day DESC LIMIT ?")?;
        st.query_map(params![day, MAX_CONSECUTIVE_SKIPS], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    let streak = flags.iter().take_while(|f| **f != 0).count() as i64;
    Ok(streak < MAX_CONSECUTIVE_SKIPS)
}

// ---- the prompt that opens by itself ----

fn state_get(c: &Connection, key: &str) -> Result<Option<String>> {
    Ok(c.query_row("SELECT value FROM journal_state WHERE key = ?", [key], |r| r.get(0)).optional()?)
}

fn state_set(c: &Connection, key: &str, value: &str) -> Result<()> {
    c.execute("INSERT INTO journal_state (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value", params![key, value])?;
    Ok(())
}

const PROMPT_SHOWN_KEY: &str = "goal_prompt_shown_day";

/// Dayflow's requestDailyGoalPromptIfNeeded and markDailyGoalPromptHandled: the morning flow for
/// `today` when the setting is on, the flow is due (`should_prompt`: no plan yet, 3 earlier days
/// with activity, fewer than 5 skips in a row) and it was not opened by itself yet today. Taking
/// it marks the day, so it opens at most once a day; the Goals tab keeps showing the flow.
pub fn take_prompt(db: &Db, today: &str, enabled: bool, cats: &[Category]) -> Result<Option<GoalsView>> {
    if !enabled {
        return Ok(None);
    }
    if state_get(&db.conn(), PROMPT_SHOWN_KEY)?.as_deref() == Some(today) {
        return Ok(None);
    }
    if !should_prompt(db, today, today)? {
        return Ok(None);
    }
    let v = view(db, today, today, cats)?;
    state_set(&db.conn(), PROMPT_SHOWN_KEY, today)?;
    Ok(Some(v))
}

// ---- checklist and evidence ----

const STOPWORDS: &[&str] = &[
    "the", "and", "for", "with", "from", "into", "onto", "that", "this", "these", "those", "then", "than", "about", "after", "before",
    "finish", "start", "work", "do", "doing", "done", "get", "make", "some", "more", "less", "today", "tomorrow", "week", "day", "time",
    "hour", "hours", "minute", "minutes", "all", "any", "have", "has", "had", "will", "just", "also", "are", "was", "were", "been", "being",
    "our", "your", "their", "them", "they", "you", "its", "new", "few", "lot", "lots", "least", "at", "on", "off", "out", "over", "under",
    "again", "very", "much", "many", "most", "own", "same", "such", "what", "which", "who", "whom", "how", "why", "when", "where", "not",
    "but", "can", "could", "should", "would", "must", "need", "try", "via", "per", "each", "every",
];

/// One suffix off (twice at most), keeping at least 4 letters: "meetings" → "meet".
fn stem(word: &str) -> String {
    let mut w = word.to_string();
    for _ in 0..2 {
        let before = w.clone();
        for suf in ["ing", "ed", "es", "s", "e"] {
            if w.ends_with(suf) && w.len() - suf.len() >= 4 {
                w.truncate(w.len() - suf.len());
                break;
            }
        }
        if w == before {
            break;
        }
    }
    w
}

fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 3)
        .map(String::from)
        .collect()
}

/// The words of a goal that are worth looking for, stemmed and without duplicates.
pub fn keywords(goal: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    words(goal).into_iter().filter(|w| !STOPWORDS.contains(&w.as_str())).map(|w| stem(&w)).filter(|w| seen.insert(w.clone())).collect()
}

fn word_matches(k: &str, w: &str) -> bool {
    k == w || (k.chars().count() >= 5 && w.starts_with(k))
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EvidenceCard {
    pub card_id: i64,
    pub title: String,
    pub start: String,
    pub end: String,
    pub secs: i64,
    /// Goal keywords found in the card.
    pub matched: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    /// Time of all matching cards.
    pub total_secs: i64,
    pub card_count: usize,
    /// The best matches, most keywords first.
    pub cards: Vec<EvidenceCard>,
}

/// Cards whose title, summary, subcategory, step-by-step text or sites mention the goal. Goals with
/// one or two keywords need one of them; longer goals need two.
pub fn evidence(goal: &str, cards: &[StoredCard], cats: &[Category]) -> Evidence {
    let keys = keywords(goal);
    if keys.is_empty() {
        return Evidence::default();
    }
    let need = if keys.len() <= 2 { 1 } else { 2 };
    let mut hits: Vec<EvidenceCard> = cards
        .iter()
        .filter(|c| matches!(stats::cat_info(&c.category, cats).kind, Kind::Focus | Kind::Distraction | Kind::Neither))
        .filter_map(|c| {
            let sites = c.app_sites.as_ref().map(|a| format!("{} {}", a.primary.clone().unwrap_or_default(), a.secondary.clone().unwrap_or_default())).unwrap_or_default();
            let text = format!("{} {} {} {} {}", c.title, c.summary, c.subcategory, c.detailed_summary, sites);
            let ws: BTreeSet<String> = words(&text).into_iter().map(|w| stem(&w)).collect();
            let matched: Vec<String> = keys.iter().filter(|k| ws.iter().any(|w| word_matches(k, w))).cloned().collect();
            (matched.len() >= need).then(|| EvidenceCard {
                card_id: c.id,
                title: c.title.clone(),
                start: c.start.clone(),
                end: c.end.clone(),
                secs: (c.end_ts - c.start_ts).max(0),
                matched,
            })
        })
        .collect();
    let total_secs = hits.iter().map(|h| h.secs).sum();
    let card_count = hits.len();
    hits.sort_by(|a, b| b.matched.len().cmp(&a.matched.len()).then(b.secs.cmp(&a.secs)));
    hits.truncate(EVIDENCE_CARDS);
    Evidence { total_secs, card_count, cards: hits }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Item {
    pub id: i64,
    pub text: String,
    pub done: bool,
    pub evidence: Evidence,
}

fn items(db: &Db, day: &str) -> Result<Vec<(i64, String, bool)>> {
    let c = db.conn();
    let mut st = c.prepare("SELECT id, text, done FROM day_goal_items WHERE day = ? ORDER BY sort_order, id")?;
    let rows = st.query_map([day], |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn clean_item(text: &str) -> Result<String> {
    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() {
        bail!("Write what you want to get done.");
    }
    Ok(t.chars().take(MAX_ITEM_CHARS).collect())
}

pub fn add_item(db: &Db, day: &str, text: &str) -> Result<i64> {
    check_day(day)?;
    let text = clean_item(text)?;
    if items(db, day)?.len() >= MAX_ITEMS {
        bail!("A day can have at most {MAX_ITEMS} goals.");
    }
    let now = now_ts();
    let c = db.conn();
    c.execute(
        "INSERT INTO day_goal_items (day, text, done, sort_order, created_at, updated_at)
         VALUES (?1, ?2, 0, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM day_goal_items WHERE day = ?1), ?3, ?3)",
        params![day, text, now],
    )?;
    Ok(c.last_insert_rowid())
}

/// Returns the item's day.
pub fn update_item(db: &Db, id: i64, text: Option<&str>, done: Option<bool>) -> Result<String> {
    let c = db.conn();
    let day: String = c.query_row("SELECT day FROM day_goal_items WHERE id = ?", [id], |r| r.get(0)).optional()?.ok_or_else(|| anyhow!("This goal no longer exists."))?;
    if let Some(t) = text {
        c.execute("UPDATE day_goal_items SET text = ?, updated_at = ? WHERE id = ?", params![clean_item(t)?, now_ts(), id])?;
    }
    if let Some(d) = done {
        c.execute("UPDATE day_goal_items SET done = ?, updated_at = ? WHERE id = ?", params![d as i64, now_ts(), id])?;
    }
    Ok(day)
}

/// Returns the item's day, if it existed.
pub fn delete_item(db: &Db, id: i64) -> Result<Option<String>> {
    let c = db.conn();
    let day: Option<String> = c.query_row("SELECT day FROM day_goal_items WHERE id = ?", [id], |r| r.get(0)).optional()?;
    c.execute("DELETE FROM day_goal_items WHERE id = ?", [id])?;
    Ok(day)
}

// ---- view ----

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GoalsView {
    pub day: String,
    pub is_today: bool,
    pub plan: Plan,
    pub progress: Progress,
    /// The previous day's result, when it had a confirmed plan.
    pub review: Option<Review>,
    pub reference: Reference,
    /// Ask for today's goals (morning flow).
    pub prompt: bool,
    pub items: Vec<Item>,
}

pub fn view(db: &Db, day: &str, today: &str, cats: &[Category]) -> Result<GoalsView> {
    check_day(day)?;
    let plan = effective_plan(db, day)?;
    let cards = db.cards_for_day(day)?;
    let (from, to) = logical_day_bounds(day).ok_or_else(|| anyhow!("Invalid day: {day}"))?;
    let progress = progress(&cards, cats, from, to);
    let yesterday = shift(day, -1)?;
    // Bound first: a guard in the match scrutinee would live through the arms and deadlock.
    let yesterday_plan = explicit_plan(&db.conn(), &yesterday)?;
    let review = match yesterday_plan {
        Some(p) if !p.is_skipped => {
            let pr = day_progress(db, &yesterday, cats)?;
            Some(Review {
                focus_met: pr.focus_secs >= p.focus_target_minutes * 60,
                distraction_met: pr.distracted_secs <= p.distraction_limit_minutes * 60,
                plan: p,
                progress: pr,
            })
        }
        _ => None,
    };
    let items = items(db, day)?
        .into_iter()
        .map(|(id, text, done)| Item { evidence: evidence(&text, &cards, cats), id, text, done })
        .collect();
    Ok(GoalsView {
        day: day.into(),
        is_today: day == today,
        prompt: should_prompt(db, day, today)?,
        reference: reference(db, day, cats)?,
        plan,
        progress,
        review,
        items,
    })
}

// ---- commands ----

type S<'a> = tauri::State<'a, Arc<AppState>>;
type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

fn view_for(state: &AppState, day: &str) -> CmdResult<GoalsView> {
    view(&state.db, day, &logical_day(now_ts()), &state.settings().categories).map_err(err)
}

#[tauri::command]
pub fn journal_get(state: S, day: String) -> CmdResult<GoalsView> {
    view_for(&state, &day)
}

/// The morning flow to open by itself (yesterday's review and today's targets), at most once a
/// day; None when it is off, not due or already shown. Called on the first use of the window
/// after the day starts.
#[tauri::command]
pub fn journal_take_prompt(state: S) -> CmdResult<Option<GoalsView>> {
    let settings = state.settings();
    take_prompt(&state.db, &logical_day(now_ts()), settings.show_goal_prompt, &settings.categories).map_err(err)
}

/// Confirms the day's targets (Dayflow "Confirm").
#[tauri::command]
pub fn journal_save_plan(state: S, day: String, focus_target_minutes: i64, distraction_limit_minutes: i64) -> CmdResult<GoalsView> {
    save_plan(&state.db, &day, focus_target_minutes, distraction_limit_minutes, false).map_err(err)?;
    view_for(&state, &day)
}

/// "Skip today": saves the current targets as skipped, which turns the progress bars off.
#[tauri::command]
pub fn journal_skip(state: S, day: String) -> CmdResult<GoalsView> {
    let p = effective_plan(&state.db, &day).map_err(err)?;
    save_plan(&state.db, &day, p.focus_target_minutes, p.distraction_limit_minutes, true).map_err(err)?;
    view_for(&state, &day)
}

#[tauri::command]
pub fn journal_add_item(state: S, day: String, text: String) -> CmdResult<GoalsView> {
    add_item(&state.db, &day, &text).map_err(err)?;
    view_for(&state, &day)
}

#[tauri::command]
pub fn journal_update_item(state: S, id: i64, text: Option<String>, done: Option<bool>) -> CmdResult<GoalsView> {
    let day = update_item(&state.db, id, text.as_deref(), done).map_err(err)?;
    view_for(&state, &day)
}

#[tauri::command]
pub fn journal_delete_item(state: S, id: i64, day: String) -> CmdResult<GoalsView> {
    let d = delete_item(&state.db, id).map_err(err)?.unwrap_or(day);
    view_for(&state, &d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ResolvedCard;
    use crate::features::insights::stats::tests::{at, card};
    use crate::types::{default_categories, ActivityCard};

    #[test]
    fn plans_are_saved_carried_forward_and_skipped() {
        let db = Db::memory();
        let p = effective_plan(&db, "2026-10-10").unwrap();
        assert_eq!((p.focus_target_minutes, p.distraction_limit_minutes, p.saved), (270, 120, false));
        save_plan(&db, "2026-10-10", 300, 60, false).unwrap();
        save_plan(&db, "2026-10-10", 330, 9999, false).unwrap();
        let p = effective_plan(&db, "2026-10-10").unwrap();
        assert_eq!((p.focus_target_minutes, p.distraction_limit_minutes, p.saved), (330, MAX_MINUTES, true));
        // A later day without its own plan uses the latest one, never as skipped.
        save_plan(&db, "2026-10-11", 200, 30, true).unwrap();
        let p = effective_plan(&db, "2026-10-14").unwrap();
        assert_eq!((p.focus_target_minutes, p.is_skipped, p.saved, p.carried_from.as_deref()), (200, false, false, Some("2026-10-11")));
        assert!(effective_plan(&db, "2026-10-11").unwrap().is_skipped);
        assert!(save_plan(&db, "not a day", 1, 1, false).is_err());
    }

    #[test]
    fn progress_uses_the_daily_numbers_and_counts_as() {
        let d = (2026, 10, 14);
        let mut cats = default_categories();
        let mut work = card(1, at(d, 9, 0), at(d, 10, 0), "Work", "Code");
        work.distractions = vec![crate::features::insights::stats::tests::distraction(at(d, 9, 20), at(d, 9, 28), "Feeds")];
        let cards = vec![work, card(2, at(d, 10, 0), at(d, 11, 0), "Personal", "Bills")];
        let (s, e) = logical_day_bounds("2026-10-14").unwrap();
        // The 8 noted minutes move from focus to distraction, as on the Daily view.
        let p = progress(&cards, &cats, s, e);
        assert_eq!((p.focus_secs, p.distracted_secs), (3600 - 480 + 3600, 480));
        assert_eq!(p.focus_categories.iter().map(|c| (c.name.as_str(), c.secs)).collect::<Vec<_>>(), vec![("Work", 3120), ("Personal", 3600)]);
        let daily = crate::features::insights::stats::compute_day("2026-10-14", &cards, &cats, s, e);
        assert_eq!((p.focus_secs, p.distracted_secs), (daily.focus_secs, daily.distracted_secs));
        // Personal set to count as neither leaves focus.
        cats[1].counts_as = Some(crate::types::CountsAs::Neither);
        let p = progress(&cards, &cats, s, e);
        assert_eq!(p.focus_secs, 3120);
        assert_eq!(p.focus_categories.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), vec!["Work"]);
    }

    #[test]
    fn prompt_opens_by_itself_once_a_day() {
        let db = Db::memory();
        let cats = default_categories();
        for day in 10..=13 {
            put(&db, at((2026, 10, day), 9, 0), at((2026, 10, day), 11, 0), "Work", "Coding", "");
        }
        assert!(take_prompt(&db, "2026-10-14", false, &cats).unwrap().is_none(), "setting off");
        let v = take_prompt(&db, "2026-10-14", true, &cats).unwrap().expect("due on the first call");
        assert!(v.prompt && v.is_today && v.day == "2026-10-14");
        assert!(take_prompt(&db, "2026-10-14", true, &cats).unwrap().is_none(), "once a day");
        // The next day it is due again, unless that day already has a plan.
        save_plan(&db, "2026-10-15", 120, 60, false).unwrap();
        assert!(take_prompt(&db, "2026-10-15", true, &cats).unwrap().is_none());
        assert!(take_prompt(&db, "2026-10-16", true, &cats).unwrap().is_some());
        // Not before 3 earlier days with activity.
        let fresh = Db::memory();
        put(&fresh, at((2026, 10, 13), 9, 0), at((2026, 10, 13), 10, 0), "Work", "Coding", "");
        assert!(take_prompt(&fresh, "2026-10-14", true, &cats).unwrap().is_none());
    }

    #[test]
    fn progress_counts_focus_and_distraction_once() {
        let d = (2026, 10, 14);
        let cats = default_categories();
        let cards = vec![
            card(1, at(d, 9, 0), at(d, 10, 0), "Work", "Code"),
            card(2, at(d, 9, 30), at(d, 10, 30), "Personal", "Bills"), // overlaps: counted from 10:00
            card(3, at(d, 11, 0), at(d, 11, 20), "Distraction", "Feeds"),
            card(4, at(d, 12, 0), at(d, 13, 0), "Idle", "Idle"),
            card(5, at(d, 13, 0), at(d, 13, 30), "System", "Processing failed"),
        ];
        let (s, e) = logical_day_bounds("2026-10-14").unwrap();
        let p = progress(&cards, &cats, s, e);
        assert_eq!(p.focus_secs, 3600 + 1800);
        assert_eq!(p.distracted_secs, 1200);
        assert_eq!(p.focus_categories.iter().map(|c| (c.name.as_str(), c.secs)).collect::<Vec<_>>(), vec![("Work", 3600), ("Personal", 1800)]);
        assert_eq!(p.distraction_categories, vec!["Distraction"]);
    }

    fn put(db: &Db, s: i64, e: i64, category: &str, title: &str, summary: &str) {
        db.insert_screenshot(s, "", 0, 0, None, None, None, None).unwrap();
        let b = db.create_batch(&db.unbatched_screenshots(0).unwrap()).unwrap();
        let card = ActivityCard {
            start_time: crate::timeutil::fmt_clock(s),
            end_time: crate::timeutil::fmt_clock(e),
            category: category.into(),
            subcategory: String::new(),
            title: title.into(),
            summary: summary.into(),
            detailed_summary: String::new(),
            distractions: None,
            app_sites: None,
        };
        db.replace_cards_in_range(s, e, &[ResolvedCard { start_ts: s, end_ts: e, card, idle_meta: None }], b, true).unwrap();
    }

    #[test]
    fn view_with_review_reference_and_prompt() {
        let db = Db::memory();
        let cats = default_categories();
        for day in 10..=13 {
            put(&db, at((2026, 10, day), 9, 0), at((2026, 10, day), 11, 0), "Work", "Coding", "");
        }
        put(&db, at((2026, 10, 13), 12, 0), at((2026, 10, 13), 15, 0), "Distraction", "Videos", "");
        save_plan(&db, "2026-10-13", 120, 60, false).unwrap();
        let v = view(&db, "2026-10-14", "2026-10-14", &cats).unwrap();
        assert!(v.prompt, "4 earlier active days, no plan for today");
        let r = v.review.unwrap();
        assert!(r.focus_met, "2 h of focus against a 2 h target");
        assert!(!r.distraction_met, "3 h distracted against a 1 h limit");
        assert_eq!(v.reference.yesterday_focus_minutes, 120);
        assert_eq!(v.reference.yesterday_distraction_minutes, 180);
        assert_eq!(v.reference.last_week_focus_minutes, 4 * 120 / 7);
        assert_eq!(v.plan.carried_from.as_deref(), Some("2026-10-13"));
        // Not on other days, and not once today's plan is saved.
        assert!(!view(&db, "2026-10-13", "2026-10-14", &cats).unwrap().prompt);
        save_plan(&db, "2026-10-14", 120, 60, true).unwrap();
        assert!(!should_prompt(&db, "2026-10-14", "2026-10-14").unwrap());
        // Five skips in a row stop the prompt.
        for d in 15..=19 {
            save_plan(&db, &format!("2026-10-{d}"), 120, 60, true).unwrap();
        }
        assert!(!should_prompt(&db, "2026-10-20", "2026-10-20").unwrap());
        save_plan(&db, "2026-10-19", 120, 60, false).unwrap();
        assert!(should_prompt(&db, "2026-10-20", "2026-10-20").unwrap());
    }

    #[test]
    fn checklist_and_evidence() {
        let db = Db::memory();
        let cats = default_categories();
        let d = (2026, 10, 14);
        put(&db, at(d, 9, 0), at(d, 10, 0), "Work", "Writing the thesis introduction", "Drafted chapter one in Word.");
        put(&db, at(d, 10, 0), at(d, 10, 30), "Work", "Thesis reading", "Read papers on screen capture.");
        put(&db, at(d, 11, 0), at(d, 11, 30), "Distraction", "Videos", "Watched videos.");
        put(&db, at(d, 12, 0), at(d, 12, 30), "Idle", "Idle", "Thesis idle text should not count.");
        let id = add_item(&db, "2026-10-14", "  Write   the thesis introduction ").unwrap();
        add_item(&db, "2026-10-14", "Call the bank").unwrap();
        assert!(add_item(&db, "2026-10-14", "   ").is_err());
        let v = view(&db, "2026-10-14", "2026-10-20", &cats).unwrap();
        assert_eq!(v.items.len(), 2);
        let thesis = &v.items[0];
        assert_eq!(thesis.text, "Write the thesis introduction");
        assert_eq!(thesis.evidence.card_count, 1, "needs two of write/thesis/introduction");
        assert_eq!(thesis.evidence.cards[0].title, "Writing the thesis introduction");
        assert_eq!(thesis.evidence.total_secs, 3600);
        assert_eq!(v.items[1].evidence.card_count, 0);
        assert_eq!(keywords("Finish the meetings notes"), vec!["meet", "note"]);
        assert_eq!(evidence("thesis", &db.cards_for_day("2026-10-14").unwrap(), &cats).card_count, 2, "one keyword is enough; idle card ignored");

        assert_eq!(update_item(&db, id, None, Some(true)).unwrap(), "2026-10-14");
        let v = view(&db, "2026-10-14", "2026-10-20", &cats).unwrap();
        assert!(v.items[0].done);
        assert_eq!(delete_item(&db, id).unwrap().as_deref(), Some("2026-10-14"));
        assert_eq!(view(&db, "2026-10-14", "2026-10-20", &cats).unwrap().items.len(), 1);
        assert!(update_item(&db, id, Some("x"), None).is_err());
    }
}
