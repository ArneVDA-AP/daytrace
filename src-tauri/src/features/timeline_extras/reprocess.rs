//! Run analysis again on request: one card's batch, a whole day, or a run of failed batches.
//!
//! Ported from Dayflow (MIT, (c) 2025 Jerry Liu):
//! - Core/Analysis/AnalysisManager.reprocessDay and Core/Recording/StorageManager+Reprocessing.swift:
//!   clear the day's cards and observations, reset its batches to pending, analyze them in order.
//!   Daytrace keeps cards the user edited and skips batches they cover.
//! - AnalysisManager.reprocessBatch: one batch again; its window rewrite replaces the card.
//! - Views/UI/RetryCoordinator.swift: "Try again" on a group of failed cards runs the batches one
//!   by one and stops at the first failure ("Stopped - earlier batch failed").
//!
//! Everything here only flips batch statuses and wakes the analysis loop (`analysis::request_retry`);
//! the loop's `process_batch` does the work, so there is never a second analysis path.

use super::cards;
use crate::db::Db;
use crate::state::AppState;
use crate::timeutil::logical_day_bounds;
use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// A batch this share covered by edited cards is not analyzed again on a day reprocess.
const EDITED_SKIP_SHARE: f64 = 0.8;
/// Statuses a day reprocess resets (Dayflow resetBatchStatuses, minus "processing").
const RESETTABLE: [&str; 3] = ["completed", "analyzed", "failed"];
const POLL: Duration = Duration::from_secs(2);
const MAX_WAIT: Duration = Duration::from_secs(3 * 3600);

#[derive(Debug, Clone)]
struct DayBatch {
    id: i64,
    start_ts: i64,
    end_ts: i64,
    status: String,
}

/// Batches inside the logical day (Dayflow fetchBatches: start and end within the day).
fn day_batches(db: &Db, day: &str) -> Result<Vec<DayBatch>> {
    let (from, to) = logical_day_bounds(day).ok_or_else(|| anyhow!("Invalid day: {day}"))?;
    let c = db.conn();
    let mut st = c.prepare(
        "SELECT id, batch_start_ts, batch_end_ts, status FROM analysis_batches
         WHERE batch_start_ts >= ?1 AND batch_end_ts <= ?2 ORDER BY batch_start_ts ASC",
    )?;
    let rows = st
        .query_map([from, to], |r| Ok(DayBatch { id: r.get(0)?, start_ts: r.get(1)?, end_ts: r.get(2)?, status: r.get(3)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

fn overlap(a: (i64, i64), b: (i64, i64)) -> i64 {
    (a.1.min(b.1) - a.0.max(b.0)).max(0)
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayPlan {
    pub day: String,
    /// Batches that will be analyzed again, in time order.
    pub batch_ids: Vec<i64>,
    /// Cards the user edited; they stay as they are.
    pub kept_edited_cards: usize,
    /// Why the day can't be reprocessed, if it can't.
    pub blocked: Option<String>,
}

/// What a day reprocess would do, without changing anything.
pub fn plan_day(db: &Db, day: &str, retention_days: u32) -> Result<DayPlan> {
    let mut plan = DayPlan { day: day.to_string(), ..Default::default() };
    let batches = day_batches(db, day)?;
    if batches.iter().any(|b| b.status == "processing") {
        plan.blocked = Some("Daytrace is analyzing this day right now. Try again when it has finished.".into());
        return Ok(plan);
    }
    let candidates: Vec<&DayBatch> = batches.iter().filter(|b| RESETTABLE.contains(&b.status.as_str())).collect();
    if candidates.is_empty() {
        plan.blocked = Some("There is nothing to analyze again on this day.".into());
        return Ok(plan);
    }
    let missing = candidates.iter().filter(|b| !cards::batch_has_frames(db, b.id).unwrap_or(false)).count();
    if missing == candidates.len() {
        plan.blocked = Some(format!(
            "The screenshots of this day were deleted (they are kept for {retention_days} days), so it can't be analyzed again."
        ));
        return Ok(plan);
    }
    if missing > 0 {
        plan.blocked = Some(format!(
            "Some screenshots of this day were already deleted (they are kept for {retention_days} days), so the day can't be analyzed again as a whole."
        ));
        return Ok(plan);
    }
    let (from, to) = logical_day_bounds(day).ok_or_else(|| anyhow!("Invalid day: {day}"))?;
    let live = db.cards_in_range(from, to, false)?;
    let edited_ids = db.user_edited_card_ids(from, to)?;
    let edited: Vec<(i64, i64)> = live.iter().filter(|c| edited_ids.contains(&c.id)).map(|c| (c.start_ts, c.end_ts)).collect();
    plan.kept_edited_cards = edited.len();
    for b in candidates {
        let span = (b.end_ts - b.start_ts).max(1);
        let covered: i64 = edited.iter().map(|e| overlap(*e, (b.start_ts, b.end_ts))).sum();
        if (covered as f64 / span as f64) < EDITED_SKIP_SHARE {
            plan.batch_ids.push(b.id);
        }
    }
    if plan.batch_ids.is_empty() {
        plan.blocked = Some("Every part of this day has cards you edited, so there is nothing to analyze again.".into());
    }
    Ok(plan)
}

/// Dayflow reprocessDay: soft-deletes the day's cards that overlap the batches to run (edited
/// cards stay), removes those batches' observations and sets them back to pending. The analysis
/// loop then runs them oldest first. Returns the plan that was applied.
pub fn reprocess_day(state: &AppState, day: &str) -> Result<DayPlan> {
    let plan = plan_day(&state.db, day, state.settings().retention_days)?;
    if let Some(why) = &plan.blocked {
        bail!("{why}");
    }
    let spans: Vec<(i64, i64)> = {
        let batches = day_batches(&state.db, day)?;
        batches.iter().filter(|b| plan.batch_ids.contains(&b.id)).map(|b| (b.start_ts, b.end_ts)).collect()
    };
    let (from, to) = logical_day_bounds(day).ok_or_else(|| anyhow!("Invalid day: {day}"))?;
    let edited = state.db.user_edited_card_ids(from, to)?;
    let doomed: Vec<i64> = state
        .db
        .cards_in_range(from, to, false)?
        .into_iter()
        .filter(|c| !edited.contains(&c.id) && spans.iter().any(|s| overlap(*s, (c.start_ts, c.end_ts.max(c.start_ts + 1))) > 0))
        .map(|c| c.id)
        .collect();
    {
        let mut c = state.db.conn();
        let tx = c.transaction()?;
        for id in &doomed {
            tx.execute("UPDATE timeline_cards SET is_deleted = 1 WHERE id = ? AND is_deleted = 0", [id])?;
        }
        for b in &plan.batch_ids {
            tx.execute("DELETE FROM observations WHERE batch_id = ?", [b])?;
            tx.execute("UPDATE analysis_batches SET status = 'pending', reason = NULL, attempts = 0 WHERE id = ?", [b])?;
        }
        tx.commit()?;
    }
    log::info!("reprocess {day}: {} batches queued, {} cards cleared, {} edited cards kept", plan.batch_ids.len(), doomed.len(), plan.kept_edited_cards);
    state.analyze_now.notify_one();
    Ok(plan)
}

/// Dayflow reprocessBatch for the batch behind a card. The card's own edit mark is removed
/// (the user asked for a new version of it); other edited cards stay protected.
pub fn reprocess_card(state: &AppState, card_id: i64) -> Result<i64> {
    let card = state.db.card(card_id)?.ok_or_else(|| anyhow!("This card no longer exists."))?;
    let batch_id = card.batch_id.ok_or_else(|| anyhow!("This card has no recording to analyze again."))?;
    let batch = state.db.batch(batch_id)?.ok_or_else(|| anyhow!("This card has no recording to analyze again."))?;
    if batch.status == "processing" {
        bail!("Daytrace is analyzing this card right now.");
    }
    if !cards::batch_has_frames(&state.db, batch_id)? {
        bail!(
            "The screenshots for this card were deleted (they are kept for {} days), so it can't be analyzed again.",
            state.settings().retention_days
        );
    }
    cards::clear_user_edit(&state.db, card_id)?;
    state.db.delete_observations(batch_id)?;
    crate::analysis::request_retry(state, batch_id)?;
    Ok(batch_id)
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchProgress {
    /// Batches waiting their turn.
    pub waiting: usize,
    /// Batches being analyzed now.
    pub running: usize,
    pub failed: usize,
    /// The analysis step of the batch being analyzed, when it belongs to this set.
    pub step: Option<String>,
}

/// Progress over a set of batches (a reprocessed day, or one card's batch).
pub fn progress(state: &AppState, batch_ids: &[i64]) -> Result<BatchProgress> {
    let mut p = BatchProgress::default();
    for id in batch_ids {
        match state.db.batch(*id)?.map(|b| b.status) {
            Some(s) if s == "pending" => p.waiting += 1,
            Some(s) if s == "processing" => p.running += 1,
            Some(s) if s == "failed" => p.failed += 1,
            _ => {}
        }
    }
    let a = state.analysis.lock().unwrap().clone();
    if a.current_batch.is_some_and(|b| batch_ids.contains(&b)) {
        p.step = a.current_step;
    }
    Ok(p)
}

// ---- sequential retry of failed batches (RetryCoordinator) ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RetryState {
    Queued,
    Running,
    Done,
    /// The batch failed again; the rest of the run was stopped.
    Failed,
    /// Not run because an earlier batch failed.
    Stopped,
    /// Waiting for an API key; the rest of the run was stopped.
    Waiting,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetryItem {
    pub batch_id: i64,
    pub state: RetryState,
    /// 1-based place among the batches still queued.
    pub position: usize,
    /// Batches in the current run.
    pub total: usize,
}

#[derive(Debug, Default)]
pub struct RetryQueue {
    order: Vec<i64>,
    states: HashMap<i64, RetryState>,
    running: bool,
}

impl RetryQueue {
    /// Adds batches to the run. Returns true when the caller must start a runner.
    pub fn enqueue(&mut self, ids: &[i64]) -> bool {
        if !self.running {
            self.order.clear();
            self.states.clear();
        }
        for id in ids {
            if matches!(self.states.get(id), Some(RetryState::Queued | RetryState::Running)) {
                continue;
            }
            if !self.order.contains(id) {
                self.order.push(*id);
            }
            self.states.insert(*id, RetryState::Queued);
        }
        let start = !self.running && self.states.values().any(|s| *s == RetryState::Queued);
        if start {
            self.running = true;
        }
        start
    }

    /// The next queued batch, marked running; None ends the runner.
    pub fn next(&mut self) -> Option<i64> {
        let id = self.order.iter().copied().find(|id| self.states.get(id) == Some(&RetryState::Queued));
        match id {
            Some(id) => {
                self.states.insert(id, RetryState::Running);
                Some(id)
            }
            None => {
                self.running = false;
                None
            }
        }
    }

    /// Records a batch's outcome; a failure stops everything still queued.
    pub fn finish(&mut self, id: i64, outcome: RetryState) {
        self.states.insert(id, outcome);
        if matches!(outcome, RetryState::Failed | RetryState::Waiting) {
            for s in self.states.values_mut() {
                if *s == RetryState::Queued {
                    *s = RetryState::Stopped;
                }
            }
        }
    }

    pub fn items(&self) -> Vec<RetryItem> {
        let total = self.order.len();
        let mut pos = 0;
        self.order
            .iter()
            .filter_map(|id| {
                let state = *self.states.get(id)?;
                if state == RetryState::Queued {
                    pos += 1;
                }
                Some(RetryItem { batch_id: *id, state, position: if state == RetryState::Queued { pos } else { 0 }, total })
            })
            .collect()
    }
}

fn queue() -> &'static Mutex<RetryQueue> {
    static Q: OnceLock<Mutex<RetryQueue>> = OnceLock::new();
    Q.get_or_init(|| Mutex::new(RetryQueue::default()))
}

pub fn retry_items() -> Vec<RetryItem> {
    queue().lock().unwrap_or_else(|e| e.into_inner()).items()
}

/// Queues failed batches for one-by-one retry. Batches that are no longer failed, or whose
/// screenshots were deleted, are skipped.
pub fn retry_batches(state: Arc<AppState>, batch_ids: &[i64]) -> Result<usize> {
    let mut ids = Vec::new();
    let mut gone = 0;
    for id in batch_ids {
        if !state.db.batch(*id)?.is_some_and(|b| b.status == "failed") || ids.contains(id) {
            continue;
        }
        if cards::batch_has_frames(&state.db, *id)? {
            ids.push(*id);
        } else {
            gone += 1;
        }
    }
    if ids.is_empty() {
        if gone > 0 {
            bail!(
                "The screenshots for this time were deleted (they are kept for {} days), so it can't be analyzed again.",
                state.settings().retention_days
            );
        }
        bail!("These batches are no longer failed.");
    }
    let start = queue().lock().unwrap_or_else(|e| e.into_inner()).enqueue(&ids);
    if start {
        tauri::async_runtime::spawn(run(state));
    }
    Ok(ids.len())
}

/// The analysis loop would keep the batch waiting for an API key (no usable fallback either).
fn missing_key(state: &AppState) -> bool {
    crate::llm::routing::waits_for_key(&state.settings())
}

async fn run(state: Arc<AppState>) {
    loop {
        let next = queue().lock().unwrap_or_else(|e| e.into_inner()).next();
        let Some(id) = next else { break };
        let outcome = run_one(&state, id).await;
        log::info!("retry of batch {id}: {outcome:?}");
        queue().lock().unwrap_or_else(|e| e.into_inner()).finish(id, outcome);
    }
}

async fn run_one(state: &AppState, id: i64) -> RetryState {
    match state.db.batch(id) {
        Ok(Some(b)) if b.status == "failed" => {}
        // Already retried by the automatic catch-up, or gone.
        _ => return RetryState::Done,
    }
    if missing_key(state) {
        return RetryState::Waiting;
    }
    if let Err(e) = crate::analysis::request_retry(state, id) {
        log::error!("retry of batch {id} could not start: {e:#}");
        return RetryState::Failed;
    }
    let started = Instant::now();
    loop {
        tokio::time::sleep(POLL).await;
        let b = match state.db.batch(id) {
            Ok(Some(b)) => b,
            _ => return RetryState::Done,
        };
        match b.status.as_str() {
            "completed" | "analyzed" | "failed_empty" | "skipped_short" => return RetryState::Done,
            "failed" => return RetryState::Failed,
            "pending" if missing_key(state) => return RetryState::Waiting,
            _ if started.elapsed() > MAX_WAIT => return RetryState::Failed,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Paths;
    use crate::db::{ResolvedCard, ERROR_TITLE, SYSTEM_CATEGORY};
    use crate::features::timeline_extras::cards::{update_card, CardEdit};
    use crate::types::{default_categories, ActivityCard, Observation};
    use chrono::{Local, TimeZone};

    /// Fakes an analysis outcome.
    fn set_status(db: &Db, id: i64, status: &str) {
        db.conn().execute("UPDATE analysis_batches SET status = ? WHERE id = ?", rusqlite::params![status, id]).unwrap();
    }

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let st = AppState::new(Paths::new(dir.path().to_path_buf()).unwrap()).unwrap();
        (dir, st)
    }

    fn at(h: u32, m: u32) -> i64 {
        Local.with_ymd_and_hms(2026, 10, 14, h, m, 0).unwrap().timestamp()
    }

    /// A 15-minute batch with frames on disk (rows not expired) and one card over it.
    fn batch_with_card(st: &AppState, start: i64, title: &str, category: &str) -> (i64, i64) {
        for i in 0..=90 {
            st.db.insert_screenshot(start + i * 10, "/x.jpg", 1, 0, Some("code"), None, None, None).unwrap();
        }
        let shots: Vec<_> = st.db.unbatched_screenshots(0).unwrap();
        let b = st.db.create_batch(&shots).unwrap();
        let card = ActivityCard {
            start_time: crate::timeutil::fmt_clock(start),
            end_time: crate::timeutil::fmt_clock(start + 900),
            category: category.into(),
            subcategory: String::new(),
            title: title.into(),
            summary: String::new(),
            detailed_summary: String::new(),
            distractions: None,
            app_sites: None,
        };
        st.db.replace_cards_in_range(start, start + 900, &[ResolvedCard { start_ts: start, end_ts: start + 900, card, idle_meta: None }], b, true).unwrap();
        st.db.insert_observations(b, &[Observation { start_ts: start, end_ts: start + 900, text: title.into() }], None).unwrap();
        set_status(&st.db, b, "completed");
        let id = st.db.cards_in_range(start, start + 900, false).unwrap().iter().find(|c| c.title == title).unwrap().id;
        (b, id)
    }

    #[test]
    fn day_reprocess_requeues_batches_and_keeps_edits() {
        let (_d, st) = state();
        let (b1, _c1) = batch_with_card(&st, at(9, 0), "Coding", "Work");
        let (b2, c2) = batch_with_card(&st, at(9, 15), "Videos", "Distraction");
        let (b3, _c3) = batch_with_card(&st, at(9, 30), "Email", "Work");
        update_card(&st.db, c2, &CardEdit { title: Some("Talk".into()), ..Default::default() }, &default_categories()).unwrap();

        let plan = plan_day(&st.db, "2026-10-14", 7).unwrap();
        assert_eq!(plan.blocked, None);
        assert_eq!(plan.batch_ids, vec![b1, b3], "the batch covered by an edited card is skipped");
        assert_eq!(plan.kept_edited_cards, 1);

        reprocess_day(&st, "2026-10-14").unwrap();
        for b in [b1, b3] {
            assert_eq!(st.db.batch(b).unwrap().unwrap().status, "pending");
        }
        assert_eq!(st.db.batch(b2).unwrap().unwrap().status, "completed");
        let live = st.db.cards_in_range(0, i64::MAX, false).unwrap();
        assert_eq!(live.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), vec!["Talk"]);
        let obs = st.db.observations_in_range(0, i64::MAX).unwrap();
        assert_eq!(obs.iter().map(|o| o.text.as_str()).collect::<Vec<_>>(), vec!["Videos"]);
        let p = progress(&st, &plan.batch_ids).unwrap();
        assert_eq!((p.waiting, p.running), (2, 0));
    }

    #[test]
    fn day_reprocess_is_blocked_without_frames_or_while_running() {
        let (_d, st) = state();
        let (b1, _) = batch_with_card(&st, at(9, 0), "Coding", "Work");
        assert!(plan_day(&st.db, "2026-10-13", 7).unwrap().blocked.is_some(), "empty day");
        set_status(&st.db, b1, "processing");
        assert!(plan_day(&st.db, "2026-10-14", 7).unwrap().blocked.unwrap().contains("right now"));
        set_status(&st.db, b1, "completed");
        st.db.expire_screenshots(i64::MAX).unwrap();
        let blocked = plan_day(&st.db, "2026-10-14", 7).unwrap().blocked.unwrap();
        assert!(blocked.contains("deleted"), "{blocked}");
        assert!(reprocess_day(&st, "2026-10-14").is_err());
        assert_eq!(st.db.cards_in_range(0, i64::MAX, false).unwrap().len(), 1, "nothing was cleared");
    }

    #[test]
    fn card_reprocess_clears_its_edit_and_requeues_its_batch() {
        let (_d, st) = state();
        let (b, c) = batch_with_card(&st, at(9, 0), "Coding", "Work");
        update_card(&st.db, c, &CardEdit { title: Some("Mine".into()), ..Default::default() }, &default_categories()).unwrap();
        assert_eq!(reprocess_card(&st, c).unwrap(), b);
        assert!(!cards::is_user_edited(&st.db, c).unwrap());
        assert_eq!(st.db.batch(b).unwrap().unwrap().status, "pending");
        assert!(st.db.observations_in_range(0, i64::MAX).unwrap().is_empty());
        assert!(reprocess_card(&st, 9999).is_err());
    }

    #[test]
    fn retry_queue_runs_in_order_and_stops_at_first_failure() {
        let mut q = RetryQueue::default();
        assert!(q.enqueue(&[3, 1, 2]));
        assert!(!q.enqueue(&[1]), "already queued, runner already started");
        let items = q.items();
        assert_eq!(items.iter().map(|i| (i.batch_id, i.position, i.total)).collect::<Vec<_>>(), vec![(3, 1, 3), (1, 2, 3), (2, 3, 3)]);
        assert_eq!(q.next(), Some(3));
        q.finish(3, RetryState::Done);
        assert_eq!(q.next(), Some(1));
        assert_eq!(q.items()[2].position, 1, "2 is now first in line");
        q.finish(1, RetryState::Failed);
        assert_eq!(q.items().iter().map(|i| i.state).collect::<Vec<_>>(), vec![RetryState::Done, RetryState::Failed, RetryState::Stopped]);
        assert_eq!(q.next(), None);
        // A new run starts fresh.
        assert!(q.enqueue(&[2]));
        assert_eq!(q.items().len(), 1);
    }

    #[test]
    fn failed_batches_are_found_for_retry() {
        let (_d, st) = state();
        let (b, _) = batch_with_card(&st, at(9, 0), ERROR_TITLE, SYSTEM_CATEGORY);
        set_status(&st.db, b, "failed");
        assert!(cards::batch_has_frames(&st.db, b).unwrap());
        let p = progress(&st, &[b]).unwrap();
        assert_eq!(p.failed, 1);
    }
}
