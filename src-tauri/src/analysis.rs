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
            // Each tick runs in its own task so a panic is logged instead of ending the loop.
            let s = state.clone();
            match tauri::async_runtime::spawn(async move { tick(&s).await }).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => log::error!("analysis tick failed: {e:#}"),
                Err(e) => {
                    log::error!("analysis tick panicked: {e}");
                    recover_interrupted(&state);
                    fail_processing_batches(&state, "Daytrace hit an internal error while analyzing this batch.");
                }
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

/// After a panic, mark the batch that was in flight as failed so it is not retried in a loop.
fn fail_processing_batches(state: &AppState, msg: &str) {
    if let Ok(pending) = state.db.batches_with_status(&["pending"], 0) {
        let current = state.analysis.lock().unwrap().current_batch;
        for b in pending.iter().filter(|b| Some(b.id) == current) {
            let _ = state.db.set_batch_status(b.id, "failed", Some(msg));
            let _ = write_error_card(state, b, msg);
        }
    }
    set_step(state, None, None);
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
    // No age limit here: a manual retry can re-queue a batch from an earlier day.
    for batch in state.db.batches_with_status(&["pending"], 0)? {
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
        log::info!("catch-up: retrying batch {} (automatic try {})", batch.id, batch.attempts + 1);
        db_increment(state, batch.id)?;
        if !process_batch(state, &batch).await? {
            break;
        }
    }
    Ok(())
}

fn db_increment(state: &AppState, id: i64) -> Result<()> {
    state.db.increment_attempts(id)
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
    // Nothing was captured because the user was away the whole time (idle guard): no model call.
    if shots.iter().all(|s| !s.has_image()) && shots.iter().all(|s| s.skip_reason.as_deref() == Some(crate::db::SKIP_IDLE)) {
        let stats = crate::batching::IdleStats { coverage: 1.0, long_idle_share: 1.0, sample_share: 1.0, largest_gap: 0 };
        write_idle_card(state, batch, &stats)?;
        db.set_batch_status(batch.id, "analyzed", Some("idle_guard"))?;
        return Ok(true);
    }
    // Without a key the batch waits (stays pending) instead of failing; it runs once a key is saved.
    let settings = state.settings();
    if settings.provider == crate::config::ProviderKind::Anthropic && crate::config::stored_key(settings.provider, &settings.base_url).is_none() {
        if batch.status != "pending" {
            db.set_batch_status(batch.id, "pending", Some("waiting_for_api_key"))?;
        }
        return Ok(false);
    }

    db.set_batch_status(batch.id, "processing", None)?;
    set_step(
        state,
        Some(batch.id),
        Some(if settings.use_batch_api { "Reading screenshots (Batch API, can take a while)" } else { "Reading screenshots" }),
    );
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
    let api_key = crate::config::stored_key(settings.provider, &settings.base_url);
    let mut client = LlmClient::new(settings.provider, &settings.base_url, &settings.model, api_key, Some(state.db.clone()))?;
    client.use_batch = settings.use_batch_api;
    client.reasoning = settings.reasoning;
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
    // Idle-shortcut cards stay out of the model's view (nothing to rewrite there) and are kept.
    let all_cards = state.db.cards_in_range(window_start, current, true)?;
    let idle_spans: Vec<(i64, i64)> = all_cards.iter().filter(|c| c.is_idle_shortcut).map(|c| (c.start_ts, c.end_ts)).collect();
    let stored: Vec<_> = all_cards.into_iter().filter(|c| !c.is_idle_shortcut).collect();
    let existing: Vec<ActivityCard> = stored.iter().map(|c| c.to_activity()).collect();
    // Context for the model: the whole span of the cards it rewrites, not just the last 45 min,
    // so the part of a long card before the window is not reduced to its summary (capped at 2 h).
    let context_start = stored.iter().map(|c| c.start_ts).min().unwrap_or(window_start).max(current - 7200).min(window_start);
    let window_obs = state.db.observations_in_range(context_start, current)?;
    let cards = match settings.pipeline {
        Pipeline::Local => llm::cards_local(&ctx, &obs, &existing).await?,
        Pipeline::Cloud => llm::cards_cloud(&ctx, &window_obs, &existing).await?,
    };

    // Models sometimes stretch a card past the evidence (e.g. to the window edge). Clamp every
    // card to the span covered by observations and the cards it was given.
    let ev_start = window_obs.iter().map(|o| o.start_ts).chain(stored.iter().map(|c| c.start_ts)).min().unwrap_or(batch.start_ts);
    let ev_end = window_obs.iter().map(|o| o.end_ts).chain(stored.iter().map(|c| c.end_ts)).max().unwrap_or(batch.end_ts);
    let anchor = (window_start + current) / 2;
    let resolved: Vec<ResolvedCard> = cards
        .into_iter()
        .filter_map(|c| {
            let Some(mut r) = ResolvedCard::from_clock(c.clone(), anchor) else {
                log::warn!("dropping card with unparsable times: {} - {}", c.start_time, c.end_time);
                return None;
            };
            // Allow a minute of slack for clock-string rounding.
            if r.start_ts < ev_start - 60 {
                r.start_ts = ev_start;
                r.card.start_time = fmt_clock(ev_start);
            }
            if r.end_ts > ev_end + 60 {
                log::info!("clamping card '{}' end {} to evidence end {}", r.card.title, r.card.end_time, fmt_clock(ev_end));
                r.end_ts = ev_end;
                r.card.end_time = fmt_clock(ev_end);
            }
            // Do not overlap the Idle cards that stay on the timeline.
            for (is, ie) in &idle_spans {
                if r.start_ts < *ie && r.end_ts > *is {
                    if r.start_ts >= *is {
                        r.start_ts = *ie;
                        r.card.start_time = fmt_clock(*ie);
                    } else {
                        r.end_ts = (*is).min(r.end_ts);
                        r.card.end_time = fmt_clock(r.end_ts);
                    }
                }
            }
            // An Idle category means the user was away; the title should say so.
            if settings.categories.iter().any(|c| c.is_idle && c.name.eq_ignore_ascii_case(&r.card.category)) {
                r.card.title = "Idle".into();
                r.card.subcategory = String::new();
            }
            (r.end_ts > r.start_ts).then_some(r)
        })
        .collect();
    if resolved.is_empty() {
        anyhow::bail!("Model returned no cards with valid times");
    }
    let mut resolved = resolved;
    if settings.pipeline == Pipeline::Cloud {
        let from = resolved.iter().map(|r| r.start_ts).min().unwrap_or(window_start);
        let to = resolved.iter().map(|r| r.end_ts).max().unwrap_or(current);
        llm::assign_detailed(&mut resolved, &state.db.observations_in_range(from, to)?);
    }
    state.db.replace_cards_in_range(window_start, current, &resolved, batch.id, true)
}

fn is_idle_card(c: &crate::db::StoredCard, idle_name: &str) -> bool {
    c.is_idle_shortcut || (c.category.eq_ignore_ascii_case(idle_name) && c.title.eq_ignore_ascii_case("idle"))
}

fn write_idle_card(state: &AppState, batch: &Batch, stats: &crate::batching::IdleStats) -> Result<()> {
    let settings = state.settings();
    let idle_name = settings.categories.iter().find(|c| c.is_idle).map(|c| c.name.clone()).unwrap_or_else(|| "Idle".into());
    let mut start = batch.start_ts;
    if let Some(prev) = state.db.last_card_ending_by(batch.start_ts)? {
        let gap = batch.start_ts - prev.end_ts;
        if is_idle_card(&prev, &idle_name) && (0..300).contains(&gap) && logical_day(prev.start_ts) == logical_day(batch.start_ts) {
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
        false,
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
        true,
    )?;
    Ok(())
}

fn enforce_retention(state: &AppState) -> Result<()> {
    let days = state.settings().retention_days as i64;
    let before = now_ts() - days * 86400;
    let paths = state.db.expire_screenshots(before)?;
    state.db.expire_text(before)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Paths, ProviderKind};
    use crate::db::SKIP_IDLE;

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let st = AppState::new(Paths::new(dir.path().to_path_buf()).unwrap()).unwrap();
        (dir, st)
    }

    #[tokio::test]
    async fn away_batch_gets_idle_card_without_model() {
        let (_d, st) = state();
        let base = now_ts() - 2000;
        // Idle-guard rows: no image, idle time below the classifier's 60 s threshold on purpose,
        // so only the "all rows are away" rule can produce the Idle card.
        for i in 0..=91 {
            st.db.insert_screenshot(base + i * 10, "", 0, 5, None, None, None, Some(SKIP_IDLE)).unwrap();
        }
        tick(&st).await.unwrap();
        let cards = st.db.cards_in_range(0, i64::MAX, false).unwrap();
        assert_eq!(cards.len(), 1);
        assert!(cards[0].is_idle_shortcut);
        assert_eq!(st.db.token_usage_since(0).unwrap(), (0, 0));
    }

    #[tokio::test]
    async fn batch_waits_for_api_key() {
        unsafe { std::env::remove_var("DAYTRACE_API_KEY") }; // test-only; no other thread reads it here
        let (_d, st) = state();
        st.settings.write().unwrap().provider = ProviderKind::Anthropic;
        let base = now_ts() - 2000;
        for i in 0..=91 {
            st.db.insert_screenshot(base + i * 10, "/nonexistent.jpg", 1, 0, Some("code"), None, None, None).unwrap();
        }
        tick(&st).await.unwrap();
        let pending = st.db.batches_with_status(&["pending"], 0).unwrap();
        assert_eq!(pending.len(), 1, "batch should wait, not fail");
        assert!(st.db.cards_in_range(0, i64::MAX, false).unwrap().is_empty(), "no error card");
    }

    #[test]
    fn rewrite_keeps_idle_cards() {
        let (_d, st) = state();
        let t = now_ts() - 3600;
        let card = |title: &str| ActivityCard {
            start_time: fmt_clock(t),
            end_time: fmt_clock(t + 900),
            category: "Idle".into(),
            subcategory: String::new(),
            title: title.into(),
            summary: String::new(),
            detailed_summary: String::new(),
            distractions: None,
            app_sites: None,
        };
        st.db.insert_screenshot(t, "", 0, 0, None, None, None, None).unwrap();
        st.db.insert_screenshot(t + 1, "", 0, 0, None, None, None, None).unwrap();
        let shots = st.db.unbatched_screenshots(0).unwrap();
        let (b1, b2) = (st.db.create_batch(&shots[..1]).unwrap(), st.db.create_batch(&shots[1..]).unwrap());
        let idle = ResolvedCard { start_ts: t, end_ts: t + 900, card: card("Idle"), idle_meta: Some(serde_json::json!({})) };
        st.db.replace_cards_in_range(t, t + 900, &[idle], b1, false).unwrap();
        let work = ResolvedCard { start_ts: t + 900, end_ts: t + 1800, card: card("Work"), idle_meta: None };
        st.db.replace_cards_in_range(t - 1800, t + 1800, &[work], b2, true).unwrap();
        let live = st.db.cards_in_range(0, i64::MAX, false).unwrap();
        assert_eq!(live.len(), 2, "idle card kept alongside the new card");
    }
}
