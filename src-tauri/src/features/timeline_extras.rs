//! Timeline extras: card editing and deleting, summary feedback, the timeline review (Focused /
//! Neutral / Distracted), reprocessing, grouped failed cards, calendar data and copying a day as
//! text. Each submodule names the Dayflow (MIT, (c) 2025 Jerry Liu) files it follows. Nothing here
//! sends data anywhere: ratings and notes stay in the local database.

pub mod cards;
pub mod reprocess;
pub mod review;

use crate::state::AppState;
use crate::timeutil::{logical_day_bounds, now_ts};
use anyhow::{anyhow, Result};
use serde::Serialize;
use std::sync::Arc;

/// Tables owned by this feature (CREATE TABLE IF NOT EXISTS ...). `timeline_review_ratings` uses
/// Dayflow's schema.
pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS card_feedback (
    card_id INTEGER PRIMARY KEY,
    direction TEXT NOT NULL,
    note TEXT NOT NULL DEFAULT '',
    card_title TEXT NOT NULL DEFAULT '',
    card_start_ts INTEGER NOT NULL DEFAULT 0,
    card_end_ts INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS timeline_review_ratings (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    start_ts INTEGER NOT NULL,
    end_ts INTEGER NOT NULL,
    rating TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_review_ratings_time ON timeline_review_ratings(start_ts, end_ts);
"#;

type S<'a> = tauri::State<'a, Arc<AppState>>;
type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

fn bounds(day: &str) -> Result<(i64, i64)> {
    logical_day_bounds(day).ok_or_else(|| anyhow!("Invalid day: {day}"))
}

/// Everything the timeline shows next to the cards of one day.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayExtras {
    pub day: String,
    /// Cards the user renamed or recategorized.
    pub edited: Vec<i64>,
    pub feedback: Vec<cards::Feedback>,
    pub reviews: Vec<review::CardReview>,
    pub review_summary: review::ReviewSummary,
    pub failed_groups: Vec<cards::FailedGroup>,
}

pub fn day_extras(state: &AppState, day: &str) -> Result<DayExtras> {
    let (from, to) = bounds(day)?;
    let day_cards = state.db.cards_for_day(day)?;
    let ids: Vec<i64> = day_cards.iter().map(|c| c.id).collect();
    let segs = review::segments_overlapping(&state.db, from.min(day_cards.iter().map(|c| c.start_ts).min().unwrap_or(from)), to.max(day_cards.iter().map(|c| c.end_ts).max().unwrap_or(to)))?;
    let mut groups = cards::group_failed(&day_cards);
    for g in groups.iter_mut() {
        g.retryable = g.batch_ids.iter().any(|b| cards::batch_has_frames(&state.db, *b).unwrap_or(false));
    }
    Ok(DayExtras {
        day: day.to_string(),
        edited: cards::edited_ids(&state.db, &day_cards)?,
        feedback: cards::feedback_for(&state.db, &ids)?,
        reviews: day_cards.iter().filter(|c| !cards::is_failed(c)).filter_map(|c| review::card_review(c, &segs)).collect(),
        review_summary: review::summary(&day_cards, &segs, from, to),
        failed_groups: groups,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayCount {
    pub day: String,
    /// Live cards other than failed ones.
    pub cards: i64,
}

/// Days in [from_day, to_day] that have timeline cards (for the calendar).
pub fn days_with_data(db: &crate::db::Db, from_day: &str, to_day: &str) -> Result<Vec<DayCount>> {
    let c = db.conn();
    let mut st = c.prepare(
        "SELECT day, COUNT(*) FROM timeline_cards WHERE is_deleted = 0 AND category != 'System'
         AND day >= ?1 AND day <= ?2 GROUP BY day ORDER BY day",
    )?;
    let rows = st
        .query_map([from_day, to_day], |r| Ok(DayCount { day: r.get(0)?, cards: r.get(1)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn needs_key(state: &AppState) -> Option<String> {
    let s = state.settings();
    (s.provider == crate::config::ProviderKind::Anthropic && crate::config::stored_key(s.provider, &s.base_url).is_none())
        .then(|| "Add an Anthropic API key in Settings first.".to_string())
}

// ---- commands ----

#[tauri::command]
pub fn te_day_extras(state: S, day: String) -> CmdResult<DayExtras> {
    day_extras(&state, &day).map_err(err)
}

#[tauri::command]
pub fn te_update_card(
    state: S,
    card_id: i64,
    title: Option<String>,
    category: Option<String>,
    subcategory: Option<String>,
) -> CmdResult<crate::db::StoredCard> {
    let cats = state.settings().categories;
    cards::update_card(&state.db, card_id, &cards::CardEdit { title, category, subcategory }, &cats).map_err(err)
}

#[tauri::command]
pub fn te_delete_card(state: S, card_id: i64) -> CmdResult<bool> {
    cards::delete_card(&state.db, card_id).map_err(err)
}

#[tauri::command]
pub fn te_set_feedback(state: S, card_id: i64, direction: Option<String>, note: Option<String>) -> CmdResult<Option<cards::Feedback>> {
    cards::set_feedback(&state.db, card_id, direction.as_deref(), note.as_deref().unwrap_or("")).map_err(err)
}

/// Rates the time of a card Focused / Neutral / Distracted (`None` clears it).
#[tauri::command]
pub fn te_rate_card(state: S, card_id: i64, rating: Option<String>) -> CmdResult<()> {
    let card = state.db.card(card_id).map_err(err)?.ok_or("This card no longer exists.")?;
    review::apply(&state.db, card.start_ts, card.end_ts, rating.as_deref()).map_err(err)
}

#[tauri::command]
pub fn te_days_with_data(state: S, from_day: String, to_day: String) -> CmdResult<Vec<DayCount>> {
    days_with_data(&state.db, &from_day, &to_day).map_err(err)
}

/// The day's timeline as plain text for the clipboard (Dayflow "Copy timeline").
#[tauri::command]
pub fn te_timeline_text(state: S, day: String) -> CmdResult<String> {
    crate::export::clipboard_text(&state.db, &day, &crate::timeutil::logical_day(now_ts())).map_err(err)
}

#[tauri::command]
pub fn te_reprocess_day_plan(state: S, day: String) -> CmdResult<reprocess::DayPlan> {
    let mut plan = reprocess::plan_day(&state.db, &day, state.settings().retention_days).map_err(err)?;
    if plan.blocked.is_none() {
        plan.blocked = needs_key(&state);
    }
    Ok(plan)
}

#[tauri::command]
pub fn te_reprocess_day(state: S, day: String) -> CmdResult<reprocess::DayPlan> {
    if let Some(why) = needs_key(&state) {
        return Err(why);
    }
    reprocess::reprocess_day(&state, &day).map_err(err)
}

/// Returns the batch that will be analyzed again.
#[tauri::command]
pub fn te_reprocess_card(state: S, card_id: i64) -> CmdResult<i64> {
    if let Some(why) = needs_key(&state) {
        return Err(why);
    }
    reprocess::reprocess_card(&state, card_id).map_err(err)
}

#[tauri::command]
pub fn te_batch_progress(state: S, batch_ids: Vec<i64>) -> CmdResult<reprocess::BatchProgress> {
    reprocess::progress(&state, &batch_ids).map_err(err)
}

/// "Try again" for a group of failed cards: one batch at a time, stopping at the first failure.
#[tauri::command]
pub fn te_retry_batches(state: S, batch_ids: Vec<i64>) -> CmdResult<usize> {
    if let Some(why) = needs_key(&state) {
        return Err(why);
    }
    reprocess::retry_batches(state.inner().clone(), &batch_ids).map_err(err)
}

#[tauri::command]
pub fn te_retry_status() -> Vec<reprocess::RetryItem> {
    reprocess::retry_items()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Paths;
    use crate::db::{ResolvedCard, ERROR_TITLE, SYSTEM_CATEGORY};
    use crate::types::ActivityCard;
    use chrono::{Local, TimeZone};

    fn card(title: &str, category: &str, s: i64, e: i64) -> ResolvedCard {
        ResolvedCard {
            start_ts: s,
            end_ts: e,
            card: ActivityCard {
                start_time: crate::timeutil::fmt_clock(s),
                end_time: crate::timeutil::fmt_clock(e),
                category: category.into(),
                subcategory: String::new(),
                title: title.into(),
                summary: String::new(),
                detailed_summary: String::new(),
                distractions: None,
                app_sites: None,
            },
            idle_meta: None,
        }
    }

    #[test]
    fn calendar_and_day_extras() {
        let dir = tempfile::tempdir().unwrap();
        let st = AppState::new(Paths::new(dir.path().to_path_buf()).unwrap()).unwrap();
        let t = |d: u32, h: u32| Local.with_ymd_and_hms(2026, 10, d, h, 0, 0).unwrap().timestamp();
        st.db.insert_screenshot(t(14, 9), "/x.jpg", 1, 0, None, None, None, None).unwrap();
        let shots = st.db.unbatched_screenshots(0).unwrap();
        let b = st.db.create_batch(&shots).unwrap();
        st.db.replace_cards_in_range(t(14, 9), t(14, 10), &[card("A", "Work", t(14, 9), t(14, 10))], b, true).unwrap();
        // 2 AM on the 16th belongs to the 15th (days start at 4 AM).
        st.db.replace_cards_in_range(t(16, 2), t(16, 3), &[card("Late", "Personal", t(16, 2), t(16, 3))], b, true).unwrap();
        st.db.replace_cards_in_range(t(17, 9), t(17, 10), &[card(ERROR_TITLE, SYSTEM_CATEGORY, t(17, 9), t(17, 10))], b, true).unwrap();
        let days = days_with_data(&st.db, "2026-10-01", "2026-10-31").unwrap();
        assert_eq!(days, vec![DayCount { day: "2026-10-14".into(), cards: 1 }, DayCount { day: "2026-10-15".into(), cards: 1 }]);
        assert!(days_with_data(&st.db, "2026-10-15", "2026-10-15").unwrap().len() == 1);

        let a = st.db.cards_for_day("2026-10-14").unwrap()[0].id;
        review::apply(&st.db, t(14, 9), t(14, 10), Some("focused")).unwrap();
        cards::set_feedback(&st.db, a, Some("up"), "").unwrap();
        let x = day_extras(&st, "2026-10-14").unwrap();
        assert_eq!(x.reviews.len(), 1);
        assert_eq!(x.feedback.len(), 1);
        assert_eq!(x.review_summary.focused_secs, 3600);
        assert_eq!(x.review_summary.unreviewed_cards, 0);
        let f = day_extras(&st, "2026-10-17").unwrap();
        assert_eq!(f.failed_groups.len(), 1);
        assert!(f.failed_groups[0].retryable, "the batch's screenshot row is not expired");
    }
}
