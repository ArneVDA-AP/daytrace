//! Analysis loop (Dayflow's AnalysisManager + LLMService.processBatch): every 60 s, batch new
//! screenshots, run each pending batch through the configured pipeline, write cards, retry failed
//! batches after a later success, and expire old screenshots.

use crate::batching::{bucketize, classify_idle, CARD_LOOKBACK, MIN_BATCH_SPAN};
use crate::config::Pipeline;
use crate::db::{Batch, ResolvedCard, ERROR_TITLE, SYSTEM_CATEGORY};
use crate::llm::{self, client::LlmClient, Ctx};
use crate::state::AppState;
use crate::timeutil::{fmt_clock, logical_day, now_ts};
use crate::types::{categories_for_llm, ActivityCard};
use anyhow::Result;
use std::sync::Arc;
use std::time::Duration;

const TICK: Duration = Duration::from_secs(60);
const LOOKBACK: i64 = 24 * 3600;
const MAX_AUTO_RETRIES: i64 = 3;

pub fn spawn(state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        recover_interrupted(&state);
        let mut last_retention = 0i64;
        loop {
            if let Err(e) = tick(&state).await {
                log::error!("analysis tick failed: {e:#}");
            }
            if now_ts() - last_retention > 3600 {
                last_retention = now_ts();
                if let Err(e) = enforce_retention(&state) {
                    log::error!("retention failed: {e:#}");
                }
            }
            tokio::select! {
                _ = tokio::time::sleep(TICK) => {}
                _ = state.analyze_now.notified() => {}
            }
        }
    });
}

/// Batches left in `processing` by a crash go back to `pending`.
fn recover_interrupted(state: &AppState) {
    if let Ok(stuck) = state.db.batches_with_status(&["processing"], 0) {
        for b in stuck {
            let _ = state.db.set_batch_status(b.id, "pending", None);
        }
    }
}

fn set_step(state: &AppState, batch: Option<i64>, step: Option<&str>) {
    let mut a = state.analysis.lock().unwrap();
    a.current_batch = batch;
    a.current_step = step.map(String::from);
}

pub async fn tick(state: &AppState) -> Result<()> {
    let since = now_ts() - LOOKBACK;
    let shots = state.db.unbatched_screenshots(since)?;
    for bucket in bucketize(&shots) {
        let id = state.db.create_batch(&bucket)?;
        log::info!("created batch {id} with {} screenshots", bucket.len());
    }

    let mut any_success = false;
    for batch in state.db.batches_with_status(&["pending"], since)? {
        if process_batch(state, &batch).await? {
            any_success = true;
        }
    }
    if any_success {
        catch_up(state).await?;
    }
    set_step(state, None, None);
    Ok(())
}

/// Retry failed batches oldest first (max 3 automatic tries each), stopping at the first failure.
async fn catch_up(state: &AppState) -> Result<()> {
    let since = now_ts() - LOOKBACK;
    for batch in state.db.batches_with_status(&["failed"], since)? {
        if batch.attempts >= MAX_AUTO_RETRIES || !state.db.has_error_card(batch.id)? {
            continue;
        }
        log::info!("catch-up: retrying batch {}", batch.id);
        if !process_batch(state, &batch).await? {
            break;
        }
    }
    Ok(())
}

/// Returns Ok(true) when the batch produced cards (or a valid idle/skip outcome).
pub async fn process_batch(state: &AppState, batch: &Batch) -> Result<bool> {
    let db = &state.db;
    let shots = db.batch_screenshots(batch.id)?;
    if shots.is_empty() {
        db.set_batch_status(batch.id, "failed_empty", None)?;
        return Ok(false);
    }
    let span = shots.last().unwrap().captured_at - shots.first().unwrap().captured_at;
    if span < MIN_BATCH_SPAN {
        db.set_batch_status(batch.id, "skipped_short", None)?;
        return Ok(false);
    }
    if let Some(stats) = classify_idle(&shots, batch.start_ts, batch.end_ts) {
        if write_idle_card(state, batch, &stats).is_ok() {
            db.set_batch_status(batch.id, "analyzed", Some("idle_shortcut_applied"))?;
            return Ok(true);
        }
    }

    db.set_batch_status(batch.id, "processing", None)?;
    db.increment_attempts(batch.id)?;
    set_step(state, Some(batch.id), Some("Reading screenshots"));
    match run_pipeline(state, batch, &shots).await {
        Ok(n) => {
            log::info!("batch {} done: {n} cards in window", batch.id);
            db.set_batch_status(batch.id, "completed", None)?;
            let mut a = state.analysis.lock().unwrap();
            a.last_success_at = Some(now_ts());
            a.last_error = None;
            Ok(true)
        }
        Err(e) => {
            let msg = format!("{e:#}");
            log::error!("batch {} failed: {msg}", batch.id);
            db.set_batch_status(batch.id, "failed", Some(&msg))?;
            write_error_card(state, batch, &msg)?;
            state.analysis.lock().unwrap().last_error = Some(msg);
            Ok(false)
        }
    }
}

async fn run_pipeline(state: &AppState, batch: &Batch, shots: &[crate::db::Shot]) -> Result<usize> {
    let settings = state.settings();
    let api_key = crate::platform::secret_get();
    let client = LlmClient::new(settings.provider, &settings.base_url, &settings.model, api_key, Some(state.db.clone()))?;
    let categories = categories_for_llm(&settings.categories);
    let ctx = Ctx {
        client: &client,
        batch_id: batch.id,
        categories: &categories,
        language: &settings.output_language,
        capture_interval: settings.capture_interval_secs as i64,
    };

    state.db.delete_observations(batch.id)?;
    let obs = match settings.pipeline {
        Pipeline::Local => llm::transcribe_local(&ctx, shots).await?,
        Pipeline::Cloud => llm::transcribe_cloud(&ctx, shots).await?,
    };
    state.db.insert_observations(batch.id, &obs, Some(&client.model))?;
    if obs.is_empty() {
        return Ok(0);
    }

    set_step(state, Some(batch.id), Some("Writing timeline cards"));
    let current = batch.end_ts;
    let window_start = current - CARD_LOOKBACK;
    let existing: Vec<ActivityCard> =
        state.db.cards_in_range(window_start, current, true)?.iter().map(|c| c.to_activity()).collect();
    let cards = match settings.pipeline {
        Pipeline::Local => llm::cards_local(&ctx, &obs, &existing).await?,
        Pipeline::Cloud => {
            let window_obs = state.db.observations_in_range(window_start, current)?;
            llm::cards_cloud(&ctx, &window_obs, &existing).await?
        }
    };
    let anchor = (window_start + current) / 2;
    let resolved: Vec<ResolvedCard> = cards
        .into_iter()
        .filter_map(|c| {
            let r = ResolvedCard::from_clock(c.clone(), anchor);
            if r.is_none() {
                log::warn!("dropping card with unparsable times: {} - {}", c.start_time, c.end_time);
            }
            r
        })
        .collect();
    if resolved.is_empty() {
        anyhow::bail!("Model returned no cards with valid times");
    }
    state.db.replace_cards_in_range(window_start, current, &resolved, batch.id)
}

fn is_idle_card(c: &crate::db::StoredCard) -> bool {
    c.category.eq_ignore_ascii_case("idle") && c.title.eq_ignore_ascii_case("idle")
}

fn write_idle_card(state: &AppState, batch: &Batch, stats: &crate::batching::IdleStats) -> Result<()> {
    let settings = state.settings();
    let idle_name = settings.categories.iter().find(|c| c.is_idle).map(|c| c.name.clone()).unwrap_or_else(|| "Idle".into());
    let mut start = batch.start_ts;
    if let Some(prev) = state.db.last_card_ending_by(batch.start_ts)? {
        let gap = batch.start_ts - prev.end_ts;
        if is_idle_card(&prev) && (0..300).contains(&gap) && logical_day(prev.start_ts) == logical_day(batch.start_ts) {
            start = prev.start_ts;
        }
    }
    let card = ActivityCard {
        start_time: fmt_clock(start),
        end_time: fmt_clock(batch.end_ts),
        category: idle_name,
        subcategory: String::new(),
        title: "Idle".into(),
        summary: "You were idle during this period.".into(),
        detailed_summary: "Idle period. Daytrace skipped activity summarization for this block.".into(),
        distractions: None,
        app_sites: None,
    };
    let meta = serde_json::json!({
        "coverage": stats.coverage, "longIdleShare": stats.long_idle_share,
        "sampleShare": stats.sample_share, "largestGap": stats.largest_gap,
    });
    state.db.replace_cards_in_range(
        start,
        batch.end_ts,
        &[ResolvedCard { start_ts: start, end_ts: batch.end_ts, card, idle_meta: Some(meta) }],
        batch.id,
    )?;
    Ok(())
}

fn friendly_error(msg: &str) -> String {
    let lower = msg.to_lowercase();
    if lower.contains("401") || lower.contains("invalid x-api-key") || lower.contains("authentication") {
        "The model provider rejected the API key.".into()
    } else if lower.contains("could not reach") || lower.contains("connection refused") || lower.contains("error sending request") {
        "Could not reach the model server.".into()
    } else if lower.contains("429") || lower.contains("rate") {
        "The model provider is rate limiting requests.".into()
    } else {
        "The model returned an unusable response.".into()
    }
}

fn write_error_card(state: &AppState, batch: &Batch, msg: &str) -> Result<()> {
    let card = ActivityCard {
        start_time: fmt_clock(batch.start_ts),
        end_time: fmt_clock(batch.end_ts),
        category: SYSTEM_CATEGORY.into(),
        subcategory: "Error".into(),
        title: ERROR_TITLE.into(),
        summary: format!("{} Your recording is saved.", friendly_error(msg)),
        detailed_summary: msg.to_string(),
        distractions: None,
        app_sites: None,
    };
    state.db.replace_cards_in_range(
        batch.start_ts,
        batch.end_ts,
        &[ResolvedCard { start_ts: batch.start_ts, end_ts: batch.end_ts, card, idle_meta: None }],
        batch.id,
    )?;
    Ok(())
}

fn enforce_retention(state: &AppState) -> Result<()> {
    let days = state.settings().retention_days as i64;
    let before = now_ts() - days * 86400;
    let paths = state.db.expire_screenshots(before)?;
    let mut removed = 0;
    for p in &paths {
        if !p.is_empty() && std::fs::remove_file(p).is_ok() {
            removed += 1;
        }
    }
    // Drop empty per-day folders.
    if let Ok(entries) = std::fs::read_dir(&state.paths.screenshots) {
        for e in entries.flatten() {
            let _ = std::fs::remove_dir(e.path()); // only succeeds when empty
        }
    }
    if removed > 0 {
        log::info!("retention: removed {removed} screenshots older than {days} days");
    }
    Ok(())
}

/// Manual retry of a failed batch from the UI.
pub fn request_retry(state: &AppState, batch_id: i64) -> Result<()> {
    state.db.reset_attempts(batch_id)?;
    state.db.set_batch_status(batch_id, "pending", None)?;
    state.analyze_now.notify_one();
    Ok(())
}
