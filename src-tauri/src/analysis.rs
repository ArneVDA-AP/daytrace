//! Analysis loop (Dayflow's AnalysisManager + LLMService.processBatch): every 60 s, batch new
//! screenshots, run each pending batch through the configured pipeline, write cards, retry failed
//! batches after a later success, and expire old screenshots.

use crate::batching::{bucketize, classify_idle, CARD_LOOKBACK, MIN_BATCH_SPAN};
use crate::config::Pipeline;
use crate::db::{Batch, ResolvedCard, SentCard, ERROR_TITLE, SYSTEM_CATEGORY};
use crate::llm::{self, Ctx};
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
        // Whatever provider is set now: frame copies and Claude transcripts left by a CLI run
        // that Quit or a crash cut short.
        let _ = tauri::async_runtime::spawn_blocking(llm::cli::startup_cleanup).await;
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
        // A batch whose images were deleted (storage limit, "delete day") has nothing to send.
        if batch.attempts >= MAX_AUTO_RETRIES || !state.db.has_error_card(batch.id)? || !state.db.batch_has_frames(batch.id)? {
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
    // The images were deleted (retention or a storage purge) and only rows without an image are
    // left: a request built from them would replace the cards with guesses. Nothing is changed.
    if !shots.iter().any(|s| s.has_image()) && shots.iter().any(|s| s.is_deleted) {
        log::info!("batch {}: its screenshots were deleted; not analyzed", batch.id);
        db.set_batch_status(batch.id, "failed_empty", Some("screenshots_deleted"))?;
        return Ok(false);
    }
    // Without a key the batch waits (stays pending) instead of failing; it runs once a key is saved
    // (or on the fallback provider, when one is set up).
    let settings = state.settings();
    if llm::routing::waits_for_key(&settings) {
        if batch.status != "pending" {
            db.set_batch_status(batch.id, "pending", Some("waiting_for_api_key"))?;
        }
        return Ok(false);
    }

    db.set_batch_status(batch.id, "processing", None)?;
    set_step(
        state,
        Some(batch.id),
        Some(if settings.use_batch_api && settings.provider == crate::config::ProviderKind::Anthropic {
            "Reading screenshots (Batch API, can take a while)"
        } else {
            "Reading screenshots"
        }),
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
    // Main provider, then the fallback provider if one is set (Dayflow's provider routing).
    let lanes = llm::routing::lanes(&settings, Some(state.db.clone()))?;
    let lanes_ref = &lanes;
    let categories = categories_for_llm(&settings.categories);
    let ctx_for = |i: usize| Ctx {
        client: &lanes_ref[i].client,
        batch_id: batch.id,
        categories: &categories,
        language: &settings.output_language,
        capture_interval: settings.capture_interval_secs as i64,
        prompts: &settings.prompt_overrides,
    };
    let current = batch.end_ts;
    let window_start = current - CARD_LOOKBACK;
    // Cards the model rewrites. Idle-shortcut cards stay out of the model's view (nothing to
    // rewrite there) and are kept; cards the user edited are treated the same way, so a rewrite
    // never undoes the edit. Which cards stay is decided again when the result is written
    // (`Db::commit_window_rewrite`), so edits and deletes made while the model works count too.
    let window_cards = || -> Result<Vec<crate::db::StoredCard>> {
        let all_cards = state.db.cards_in_range(window_start, current, true)?;
        let edited = state.db.user_edited_card_ids(window_start, current)?;
        Ok(all_cards.into_iter().filter(|c| !c.is_idle_shortcut && !edited.contains(&c.id)).collect())
    };
    // Context for the model: the whole span of the cards it rewrites, not just the last 45 min,
    // so the part of a long card before the window is not reduced to its summary (capped at 2 h).
    let context_start = |stored: &[crate::db::StoredCard]| stored.iter().map(|c| c.start_ts).min().unwrap_or(window_start).max(current - 7200).min(window_start);

    state.db.delete_observations(batch.id)?;
    let mut active = 0usize;

    // Claude Code / Codex CLI: screenshots to cards in one call (Dayflow's one-step timeline).
    // When it fails, the batch falls through to the two-step path below.
    let mut one_step: Option<(Vec<ActivityCard>, Vec<crate::db::StoredCard>)> = None;
    let mut one_step_window: Option<(i64, i64)> = None;
    let mut skipped_main: Option<String> = None;
    if lanes[0].is_primary && lanes[0].client.is_cli() {
        set_step(state, Some(batch.id), Some("Writing timeline cards"));
        let stored = window_cards()?;
        match llm::one_step::generate_timeline(&ctx_for(0), shots, &stored).await {
            Ok(r) => {
                state.db.insert_observations(batch.id, &r.observations, Some(&r.model))?;
                one_step = Some((r.cards, stored));
                one_step_window = Some(r.window);
            }
            Err(e) => {
                log::warn!("one-step timeline failed for batch {}: {e:#}", batch.id);
                // Signed out, not installed, out of usage: the same CLI would fail the same way.
                // A timeout already spent the whole run; the two-step path on the same CLI would
                // spend up to two more. Both go to the fallback provider, or fail the batch.
                if llm::failure::classify(&format!("{e:#}")).kind.is_actionable() || llm::cli::is_timeout(&e) {
                    if lanes.len() < 2 {
                        return Err(e);
                    }
                    active = 1;
                    skipped_main = Some(format!("{e:#}"));
                }
                set_step(state, Some(batch.id), Some("Reading screenshots"));
            }
        }
    }

    let from_one_step = one_step.is_some();
    // When the main CLI was skipped, a fallback failure also names the main provider's problem.
    let note_main = |e: anyhow::Error| match &skipped_main {
        Some(m) => anyhow::anyhow!("{e:#}\n\nThe main provider failed first: {m}"),
        None => e,
    };
    // `stored`: the previous cards the model was shown (and had to cover).
    let (cards, stored, window_obs, cards_pipeline) = match one_step {
        Some((cards, stored)) => {
            let window_obs = state.db.observations_in_range(context_start(&stored), current)?;
            (cards, stored, window_obs, None)
        }
        None => {
            let obs = llm::routing::run_stage("transcribe", lanes.len(), &mut active, |i| async move {
                let ctx = ctx_for(i);
                match lanes_ref[i].pipeline {
                    Pipeline::Local => llm::transcribe_local(&ctx, shots).await,
                    Pipeline::Cloud => llm::transcribe_cloud(&ctx, shots).await,
                }
            })
            .await
            .map_err(note_main)?;
            state.db.insert_observations(batch.id, &obs, Some(&lanes[active].client.model))?;
            if obs.is_empty() {
                return Ok(0);
            }

            set_step(state, Some(batch.id), Some("Writing timeline cards"));
            let stored = window_cards()?;
            let existing: Vec<ActivityCard> = stored.iter().map(|c| c.to_activity()).collect();
            let window_obs = state.db.observations_in_range(context_start(&stored), current)?;
            let (obs_ref, existing_ref, window_ref) = (&obs, &existing, &window_obs);
            let cards = llm::routing::run_stage("generate_cards", lanes.len(), &mut active, |i| async move {
                let ctx = ctx_for(i);
                match lanes_ref[i].pipeline {
                    Pipeline::Local => llm::cards_local(&ctx, obs_ref, existing_ref).await,
                    Pipeline::Cloud => llm::cards_cloud(&ctx, window_ref, existing_ref).await,
                }
            })
            .await
            .map_err(note_main)?;
            (cards, stored, window_obs, Some(lanes[active].pipeline))
        }
    };

    // Models sometimes stretch a card past the evidence (e.g. to the window edge). Clamp every
    // card to the span covered by observations and the cards it was given.
    // The one-step window (validated as covered) counts as evidence too.
    let ev_start = window_obs.iter().map(|o| o.start_ts).chain(stored.iter().map(|c| c.start_ts)).chain(one_step_window.map(|w| w.0)).min().unwrap_or(batch.start_ts);
    let ev_end = window_obs.iter().map(|o| o.end_ts).chain(stored.iter().map(|c| c.end_ts)).chain(one_step_window.map(|w| w.1)).max().unwrap_or(batch.end_ts);
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
    // The Detailed mode writes no detail log itself; it is built from the transcription. The
    // one-step cards carry their own.
    let detail_obs = if !from_one_step && cards_pipeline == Some(Pipeline::Cloud) {
        let from = resolved.iter().map(|r| r.start_ts).min().unwrap_or(window_start);
        let to = resolved.iter().map(|r| r.end_ts).max().unwrap_or(current);
        Some(state.db.observations_in_range(from, to)?)
    } else {
        None
    };
    let sent: Vec<SentCard> = stored.iter().map(|c| SentCard { id: c.id, start_ts: c.start_ts, end_ts: c.end_ts }).collect();
    let batch_id = batch.id;
    state.db.commit_window_rewrite(window_start, current, batch.id, &sent, move |avoid| {
        // Cards that stay (Idle, edited, also while the model ran) and time the user deleted
        // meanwhile are cut out of the model's cards; a card around such a span is split.
        let mut out: Vec<ResolvedCard> = split_around(resolved, avoid)
            .into_iter()
            .map(|(mut piece, cut)| {
                if cut && from_one_step {
                    piece.card.detailed_summary = clip_detail_lines(&piece.card.detailed_summary, piece.start_ts, piece.end_ts, anchor, &piece.card.summary);
                }
                piece
            })
            .collect();
        if let Some(obs) = &detail_obs {
            llm::assign_detailed(&mut out, obs);
        }
        if out.is_empty() {
            log::info!("batch {batch_id}: every new card fell on cards that stay or were deleted meanwhile; nothing written");
        }
        out
    })
}

/// Pieces shorter than this are dropped when a card is split.
const MIN_PIECE_SECS: i64 = 60;

/// Cuts the time inside `avoid` out of each card: a card around such a span becomes a card
/// before it and a card after it (with the same text). Returns each piece and whether its card
/// was cut; cut pieces shorter than a minute are dropped.
pub(crate) fn split_around(cards: Vec<ResolvedCard>, avoid: &[(i64, i64)]) -> Vec<(ResolvedCard, bool)> {
    let mut spans: Vec<(i64, i64)> = avoid.iter().copied().filter(|(s, e)| e > s).collect();
    spans.sort_unstable();
    let mut out = Vec::new();
    for rc in cards {
        let mut pieces = vec![(rc.start_ts, rc.end_ts)];
        for &(s, e) in &spans {
            pieces = pieces
                .into_iter()
                .flat_map(|(a, b)| {
                    if e <= a || s >= b {
                        return vec![(a, b)];
                    }
                    let mut v = Vec::new();
                    if s > a {
                        v.push((a, s));
                    }
                    if e < b {
                        v.push((e, b));
                    }
                    v
                })
                .collect();
        }
        if pieces == [(rc.start_ts, rc.end_ts)] {
            out.push((rc, false));
            continue;
        }
        for (a, b) in pieces {
            if b - a < MIN_PIECE_SECS {
                continue;
            }
            let mut p = rc.clone();
            p.start_ts = a;
            p.end_ts = b;
            p.card.start_time = fmt_clock(a);
            p.card.end_time = fmt_clock(b);
            out.push((p, true));
        }
    }
    out
}

/// The detail lines ("9:00 AM - 9:07 AM: ...") of a one-step card that start inside a piece of
/// it; lines without readable times stay. Falls back to `summary` when nothing is left.
fn clip_detail_lines(detail: &str, from: i64, to: i64, anchor: i64, summary: &str) -> String {
    let kept: Vec<&str> = detail
        .lines()
        .filter(|line| match llm::one_step::parse_detail_line(line) {
            Some((start, _, _)) => crate::timeutil::resolve_card_range(&start, &start, anchor).map(|(s, _)| s >= from - 60 && s < to).unwrap_or(true),
            None => true,
        })
        .collect();
    let text = kept.join("\n");
    if text.trim().is_empty() { summary.to_string() } else { text }
}

fn is_idle_card(c: &crate::db::StoredCard, idle_name: &str) -> bool {
    c.is_idle_shortcut || (c.category.eq_ignore_ascii_case(idle_name) && c.title.eq_ignore_ascii_case("idle"))
}

fn write_idle_card(state: &AppState, batch: &Batch, stats: &crate::batching::IdleStats) -> Result<()> {
    let settings = state.settings();
    let idle_name = settings.categories.iter().find(|c| c.is_idle).map(|c| c.name.clone()).unwrap_or_else(|| "Idle".into());
    let mut start = batch.start_ts;
    // Merge with the card right before only when it is an Idle card the user did not edit; an
    // edited card in between ends the Idle stretch.
    if let Some(prev) = state.db.last_card_ending_by(batch.start_ts)? {
        let gap = batch.start_ts - prev.end_ts;
        let edited = state.db.user_edited_card_ids(prev.start_ts, prev.end_ts.max(prev.start_ts + 1))?.contains(&prev.id);
        if !edited && is_idle_card(&prev, &idle_name) && (0..300).contains(&gap) && logical_day(prev.start_ts) == logical_day(batch.start_ts) {
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

/// The failed card's sentence (Dayflow TimelineFailureClassifier).
fn friendly_error(msg: &str, provider: crate::config::ProviderKind) -> String {
    llm::failure::classify(msg).card_message(llm::failure::provider_name(provider))
}

fn write_error_card(state: &AppState, batch: &Batch, msg: &str) -> Result<()> {
    let card = ActivityCard {
        start_time: fmt_clock(batch.start_ts),
        end_time: fmt_clock(batch.end_ts),
        category: SYSTEM_CATEGORY.into(),
        subcategory: "Error".into(),
        title: ERROR_TITLE.into(),
        summary: format!("{} Your recording is saved.", friendly_error(msg, state.settings().provider)),
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

/// After the retention period: screenshot files are deleted and model call logs lose their
/// request and response text. Cards, observations and token counts stay; screenshot rows (app
/// and window title) stay for `SCREENSHOT_ROW_DAYS`.
fn enforce_retention(state: &AppState) -> Result<()> {
    let days = state.settings().retention_days as i64;
    let now = now_ts();
    let before = now - days * 86400;
    let paths = state.db.expire_screenshots(before)?;
    state.db.expire_text(before)?;
    let purged = state.db.purge_screenshot_rows(now - crate::db::SCREENSHOT_ROW_DAYS * 86400)?;
    if purged > 0 {
        log::info!("retention: deleted {purged} screenshot rows older than {} days", crate::db::SCREENSHOT_ROW_DAYS);
    }
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
    // Files the rows do not know about (after a database restore or reset, or a failed delete):
    // whole day folders past the retention period, and stray files older than the newest backup.
    crate::features::storage::sweep_screenshot_files(state, before);
    crate::features::storage::remove_old_damaged_databases(&state.paths.root, now);
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
    fn retention_keeps_text_the_features_read() {
        let (dir, st) = state();
        let day = 86400;
        let now = now_ts();
        let shot = |at: i64, name: &str| {
            let p = dir.path().join(name);
            std::fs::write(&p, b"jpg").unwrap();
            st.db.insert_screenshot(at, &p.to_string_lossy(), 3, 0, Some("WINWORD"), Some("Report.docx"), None, None).unwrap();
            p
        };
        let ancient = shot(now - 400 * day, "ancient.jpg");
        let old = shot(now - 10 * day, "old.jpg");
        let fresh = shot(now - 3600, "fresh.jpg");
        let shots = st.db.unbatched_screenshots(0).unwrap();
        let b_ancient = st.db.create_batch(&shots[..1]).unwrap();
        let b_old = st.db.create_batch(&shots[1..2]).unwrap();
        st.db.create_batch(&shots[2..]).unwrap();
        let obs = |at: i64| vec![crate::types::Observation { start_ts: at, end_ts: at + 60, text: "Editing the report".into() }];
        st.db.insert_observations(b_ancient, &obs(now - 400 * day), Some("m")).unwrap();
        st.db.insert_observations(b_old, &obs(now - 10 * day), Some("m")).unwrap();
        st.db.log_llm_call(Some(b_old), "transcribe", "m", 10, "{\"req\":1}", Some("{\"res\":1}"), None, (Some(1000), Some(50)));
        st.db.conn().execute("UPDATE llm_calls SET created_at = datetime(?, 'unixepoch')", [now - 10 * day]).unwrap();
        st.db.log_llm_call(None, "chat", "m", 10, "{\"req\":2}", Some("{\"res\":2}"), None, (Some(200), Some(20)));

        enforce_retention(&st).unwrap();

        assert!(!ancient.exists() && !old.exists() && fresh.exists(), "images past the retention period are deleted");
        assert_eq!(st.db.observations_in_range(0, i64::MAX).unwrap().len(), 2, "observations stay");
        assert_eq!(st.db.token_usage_since(0).unwrap(), (1200, 70), "token counts stay");
        let texts: Vec<(Option<String>, Option<String>)> = {
            let c = st.db.conn();
            let mut q = c.prepare("SELECT request, response FROM llm_calls ORDER BY id").unwrap();
            q.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap()
        };
        assert_eq!(texts[0], (None, None), "old request and response text cleared");
        assert_eq!(texts[1].0.as_deref(), Some("{\"req\":2}"), "recent call logs keep their text");
        // The 10-day-old row stays without an image (program statistics read it); the
        // 400-day-old one is gone with its batch link, while its batch and observation stay.
        let rows: Vec<(i64, String, i64)> = {
            let c = st.db.conn();
            let mut q = c.prepare("SELECT captured_at, file_path, is_deleted FROM screenshots ORDER BY captured_at").unwrap();
            q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?))).unwrap().collect::<rusqlite::Result<_>>().unwrap()
        };
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], (now - 10 * day, String::new(), 1));
        assert!(st.db.batch_screenshots(b_ancient).unwrap().is_empty());
        assert!(st.db.batch(b_ancient).unwrap().is_some());
        assert_eq!(st.db.batch_screenshots(b_old).unwrap().len(), 1);
        // A second pass changes nothing.
        enforce_retention(&st).unwrap();
        assert_eq!(st.db.token_usage_since(0).unwrap(), (1200, 70));
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

/// Window rewrites against a scripted OpenAI-compatible server on localhost (no model is called):
/// purged batches, and edits and deletes made while the model works.
#[cfg(test)]
mod rewrite_tests {
    use super::*;
    use crate::config::{Paths, Pipeline, ProviderKind};
    use crate::features::timeline_extras::{cards, reprocess};
    use crate::types::Observation;
    use serde_json::json;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Mutex;

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let st = AppState::new(Paths::new(dir.path().to_path_buf()).unwrap()).unwrap();
        (dir, st)
    }

    fn reply(content: &str) -> String {
        json!({"choices": [{"message": {"content": content}, "finish_reason": "stop"}], "usage": {"prompt_tokens": 10, "completion_tokens": 5}}).to_string()
    }

    /// Answers each request with the next reply; `hook(n)` runs before the n-th reply (0-based),
    /// while the request is "in flight". Records the request bodies.
    fn server(replies: Vec<String>, hook: Box<dyn Fn(usize) + Send>) -> (String, Arc<Mutex<Vec<serde_json::Value>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let seen = bodies.clone();
        std::thread::spawn(move || {
            for (n, reply) in replies.into_iter().enumerate() {
                let Ok((mut stream, _)) = listener.accept() else { return };
                let mut buf = Vec::new();
                let mut chunk = [0u8; 65536];
                let header_end = loop {
                    let k = stream.read(&mut chunk).unwrap_or(0);
                    if k == 0 {
                        break None;
                    }
                    buf.extend_from_slice(&chunk[..k]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break Some(i + 4);
                    }
                };
                let Some(header_end) = header_end else { continue };
                let head = String::from_utf8_lossy(&buf[..header_end]).to_lowercase();
                let len: usize = head.lines().find_map(|l| l.strip_prefix("content-length:").map(|v| v.trim().parse().unwrap_or(0))).unwrap_or(0);
                while buf.len() < header_end + len {
                    let k = stream.read(&mut chunk).unwrap_or(0);
                    if k == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..k]);
                }
                seen.lock().unwrap().push(serde_json::from_slice(&buf[header_end..(header_end + len).min(buf.len())]).unwrap_or(serde_json::Value::Null));
                hook(n);
                let resp = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        (url, bodies)
    }

    fn use_server(st: &AppState, url: &str) {
        let mut s = st.settings.write().unwrap();
        s.provider = ProviderKind::OpenaiCompatible;
        s.base_url = url.into();
        s.model = "m".into();
        s.pipeline = Pipeline::Cloud;
    }

    fn act(title: &str, s: i64, e: i64) -> ActivityCard {
        ActivityCard {
            start_time: fmt_clock(s),
            end_time: fmt_clock(e),
            category: "Work".into(),
            subcategory: String::new(),
            title: title.into(),
            summary: format!("{title} summary"),
            detailed_summary: String::new(),
            distractions: None,
            app_sites: None,
        }
    }

    /// 91 frames over [start, start+900]; the first `idle_rows` are away rows without an image.
    fn batch(st: &AppState, dir: &std::path::Path, start: i64, idle_rows: usize) -> i64 {
        let img = crate::imaging::encode_jpeg(&image::RgbImage::from_pixel(64, 36, image::Rgb([20, 90, 200])), 80).unwrap();
        for i in 0..=90i64 {
            let at = start + i * 10;
            if (i as usize) < idle_rows {
                st.db.insert_screenshot(at, "", 0, 200, None, None, None, Some(crate::db::SKIP_IDLE)).unwrap();
            } else {
                let f = dir.join(format!("f{at}.jpg"));
                std::fs::write(&f, &img).unwrap();
                st.db.insert_screenshot(at, &f.display().to_string(), img.len() as i64, 0, Some("Code"), None, None, None).unwrap();
            }
        }
        let shots = st.db.unbatched_screenshots(0).unwrap();
        st.db.create_batch(&shots).unwrap()
    }

    /// Stores `c` as the card of batch `b` and marks the batch completed.
    fn put(st: &AppState, b: i64, c: ActivityCard, s: i64, e: i64) -> i64 {
        st.db.replace_cards_in_range(s, e, &[ResolvedCard { start_ts: s, end_ts: e, card: c.clone(), idle_meta: None }], b, true).unwrap();
        st.db.set_batch_status(b, "completed", None).unwrap();
        st.db.cards_in_range(s, e, false).unwrap().into_iter().find(|x| x.title == c.title).unwrap().id
    }

    fn base() -> i64 {
        (now_ts() - 4 * 3600) / 60 * 60
    }

    fn obs(at: i64, to: i64, text: &str) -> Vec<Observation> {
        vec![Observation { start_ts: at, end_ts: to, text: text.into() }]
    }

    fn live(st: &AppState) -> Vec<crate::db::StoredCard> {
        st.db.cards_in_range(0, i64::MAX, false).unwrap()
    }

    fn rename(st: &AppState, id: i64, title: &str) {
        cards::update_card(&st.db, id, &cards::CardEdit { title: Some(title.into()), ..Default::default() }, &st.settings().categories).unwrap();
    }

    fn segments(text: &str) -> String {
        reply(&json!({"segments": [{"start": "00:00:00", "end": "00:15:00", "description": text}]}).to_string())
    }

    fn cards_reply(cards: &[(&str, i64, i64)]) -> String {
        let v: Vec<serde_json::Value> = cards
            .iter()
            .map(|(t, s, e)| json!({"startTime": fmt_clock(*s), "endTime": fmt_clock(*e), "category": "Work", "subcategory": "", "title": t, "summary": "S", "distractions": [], "appSites": null}))
            .collect();
        reply(&serde_json::Value::Array(v).to_string())
    }

    /// Storage "delete day" or the size limit removed a batch's images; one away row is left.
    /// The batch is not analyzed again (no text-only request replaces its card), and a batch
    /// that keeps some images is analyzed from those.
    #[tokio::test]
    async fn purged_batch_is_not_analyzed_again() {
        let (d, st) = state();
        let t0 = base();
        let b = batch(&st, d.path(), t0, 5);
        let good = put(&st, b, act("Writing the quarterly report", t0, t0 + 900), t0, t0 + 900);
        let purged = st.db.expire_screenshots_between(t0 - 10, t0 + 1000, i64::MAX).unwrap();
        assert_eq!(purged.len(), 86);
        for (p, _) in &purged {
            std::fs::remove_file(p).unwrap();
        }
        let rows = st.db.batch_screenshots(b).unwrap();
        assert!(rows.iter().all(|s| !s.has_image()), "purged rows have no image");
        assert!(rows.iter().filter(|s| s.is_deleted).all(|s| s.file_path.is_empty()), "a purge clears the path");
        assert!(!st.db.batch_has_frames(b).unwrap());
        assert!(reprocess::reprocess_card(&st, good).unwrap_err().to_string().contains("deleted"));

        // Even when such a batch is queued (a retry, a pending batch past retention): no request,
        // and the card stays.
        let (url, bodies) = server(vec![], Box::new(|_| {}));
        use_server(&st, &url);
        st.db.set_batch_status(b, "pending", None).unwrap();
        let bt = st.db.batch(b).unwrap().unwrap();
        assert!(!process_batch(&st, &bt).await.unwrap());
        assert!(bodies.lock().unwrap().is_empty(), "nothing was sent");
        assert_eq!(st.db.batch(b).unwrap().unwrap().status, "failed_empty");
        assert!(live(&st).iter().any(|c| c.id == good), "the good card stays");

        // A failed batch whose images are gone is not retried by the catch-up.
        st.db.set_batch_status(b, "failed", None).unwrap();
        write_error_card(&st, &bt, "HTTP 529: overloaded").unwrap();
        catch_up(&st).await.unwrap();
        assert_eq!(st.db.batch(b).unwrap().unwrap().attempts, 0);

        // Partly purged: the request uses the images that are left (15 of the 30), not an even
        // pick over all 91 rows that finds a third of them.
        let t1 = t0 + 1000;
        let b2 = batch(&st, d.path(), t1, 0);
        put(&st, b2, act("Coding", t1, t1 + 900), t1, t1 + 900);
        let gone = st.db.expire_screenshots_between(t1, t1 + 610, i64::MAX).unwrap();
        assert_eq!(gone.len(), 61);
        assert!(st.db.batch_has_frames(b2).unwrap());
        let (url, bodies) = server(vec![segments("VS Code: coding"), cards_reply(&[("Coding", t1, t1 + 900)])], Box::new(|_| {}));
        use_server(&st, &url);
        let bt2 = st.db.batch(b2).unwrap().unwrap();
        assert!(process_batch(&st, &bt2).await.unwrap());
        let images = bodies.lock().unwrap()[0].to_string().matches("\"type\":\"image_url\"").count();
        assert_eq!(images, 15);
    }

    /// The user renames a card while the cards request runs: the edit stays, and the model's
    /// card over the same time is cut around it instead of overlapping it.
    #[tokio::test]
    async fn edit_during_the_cards_request_is_respected() {
        let (d, st) = state();
        let t0 = base();
        let b1 = batch(&st, d.path(), t0, 0);
        let x = put(&st, b1, act("Coding", t0, t0 + 900), t0, t0 + 900);
        st.db.insert_observations(b1, &obs(t0, t0 + 900, "coding"), None).unwrap();
        let b2 = batch(&st, d.path(), t0 + 910, 0);
        let db = st.db.clone();
        let cats = st.settings().categories;
        let (url, _) = server(
            vec![segments("VS Code: more coding"), cards_reply(&[("Coding session", t0, t0 + 1810)])],
            Box::new(move |n| {
                if n == 1 {
                    cards::update_card(&db, x, &cards::CardEdit { title: Some("Code review for Jan".into()), ..Default::default() }, &cats).unwrap();
                }
            }),
        );
        use_server(&st, &url);
        let bt = st.db.batch(b2).unwrap().unwrap();
        assert!(process_batch(&st, &bt).await.unwrap());
        let cards = live(&st);
        let edited = cards.iter().find(|c| c.id == x).expect("edited card kept");
        assert_eq!(edited.title, "Code review for Jan");
        assert!(!cards.iter().any(|c| c.id != x && c.start_ts < edited.end_ts && c.end_ts > edited.start_ts), "{cards:?}");
        let rest = cards.iter().find(|c| c.title == "Coding session").expect("the new time has a card");
        // Card times are whole minutes ("9:30 AM"): 1810 s reads as 1800.
        assert_eq!((rest.start_ts, rest.end_ts), (t0 + 900, t0 + 1800));
    }

    /// A model card from before an edited card to past it is split around it: the part after it
    /// (the new batch) is kept.
    #[tokio::test]
    async fn card_around_an_edited_card_is_split() {
        let (d, st) = state();
        let t0 = base();
        let b1 = batch(&st, d.path(), t0, 0);
        put(&st, b1, act("Planning", t0, t0 + 900), t0, t0 + 900);
        st.db.insert_observations(b1, &obs(t0, t0 + 900, "planning in Notion"), None).unwrap();
        let b2 = batch(&st, d.path(), t0 + 910, 0);
        let meeting = put(&st, b2, act("Meeting", t0 + 900, t0 + 1810), t0 + 900, t0 + 1810);
        st.db.insert_observations(b2, &obs(t0 + 910, t0 + 1810, "Teams call"), None).unwrap();
        rename(&st, meeting, "1:1 with manager");
        let b3 = batch(&st, d.path(), t0 + 1820, 0);
        let end = st.db.batch(b3).unwrap().unwrap().end_ts;
        let (url, _) = server(vec![segments("Notion: planning again"), cards_reply(&[("Planning the quarter", t0, end)])], Box::new(|_| {}));
        use_server(&st, &url);
        let bt = st.db.batch(b3).unwrap().unwrap();
        assert!(process_batch(&st, &bt).await.unwrap());
        let mut got: Vec<(String, i64, i64)> = live(&st).into_iter().map(|c| (c.title, c.start_ts - t0, c.end_ts - t0)).collect();
        got.sort_by_key(|g| g.1);
        assert_eq!(
            got,
            vec![
                ("Planning the quarter".to_string(), 0, 900),
                ("1:1 with manager".to_string(), 900, 1810),
                ("Planning the quarter".to_string(), 1810, (end - t0) / 60 * 60),
            ]
        );
        // Each piece's step-by-step log holds only its own observations.
        let after = live(&st).into_iter().find(|c| c.start_ts == t0 + 1810).unwrap();
        assert!(after.detailed_summary.contains("planning again") && !after.detailed_summary.contains("Teams call"), "{}", after.detailed_summary);
    }

    /// A card deleted while its window is rewritten is not brought back by the model's output,
    /// which had to cover it.
    #[tokio::test]
    async fn card_deleted_during_the_call_stays_deleted() {
        let (d, st) = state();
        let t0 = base();
        let b1 = batch(&st, d.path(), t0, 0);
        let gone = put(&st, b1, act("Watching videos", t0, t0 + 900), t0, t0 + 900);
        st.db.insert_observations(b1, &obs(t0, t0 + 900, "YouTube"), None).unwrap();
        let b2 = batch(&st, d.path(), t0 + 910, 0);
        let db = st.db.clone();
        let (url, _) = server(
            vec![segments("Notion: planning"), cards_reply(&[("Watching videos", t0, t0 + 900), ("Planning", t0 + 900, t0 + 1810)])],
            Box::new(move |n| {
                if n == 1 {
                    assert!(cards::delete_card(&db, gone).unwrap());
                }
            }),
        );
        use_server(&st, &url);
        let bt = st.db.batch(b2).unwrap().unwrap();
        assert!(process_batch(&st, &bt).await.unwrap());
        let got: Vec<(String, i64, i64)> = live(&st).into_iter().map(|c| (c.title, c.start_ts - t0, c.end_ts - t0)).collect();
        assert_eq!(got, vec![("Planning".to_string(), 900, 1800)]);
    }

    /// An Idle batch after a short edited card does not merge with the Idle card before it.
    #[test]
    fn idle_does_not_stretch_over_an_edited_card() {
        let (d, st) = state();
        let t0 = base();
        let b1 = batch(&st, d.path(), t0, 0);
        let mut idle = act("Idle", t0, t0 + 900);
        idle.category = "Idle".into();
        st.db.replace_cards_in_range(t0, t0 + 900, &[ResolvedCard { start_ts: t0, end_ts: t0 + 900, card: idle, idle_meta: Some(json!({})) }], b1, false).unwrap();
        let b2 = batch(&st, d.path(), t0 + 1000, 0);
        let call = put(&st, b2, act("Quick call", t0 + 900, t0 + 1080), t0 + 900, t0 + 1080);
        rename(&st, call, "Call with plumber");
        let away = Batch { id: b2, start_ts: t0 + 1150, end_ts: t0 + 2050, status: "pending".into(), reason: None, attempts: 0 };
        let stats = crate::batching::IdleStats { coverage: 1.0, long_idle_share: 1.0, sample_share: 1.0, largest_gap: 0 };
        write_idle_card(&st, &away, &stats).unwrap();
        let mut got: Vec<(String, i64, i64)> = live(&st).into_iter().map(|c| (c.title, c.start_ts - t0, c.end_ts - t0)).collect();
        got.sort_by_key(|g| g.1);
        assert_eq!(got, vec![("Idle".to_string(), 0, 900), ("Call with plumber".to_string(), 900, 1080), ("Idle".to_string(), 1150, 2050)]);
        // Without the edited card in between, the Idle cards still merge.
        let b3 = batch(&st, d.path(), t0 + 3000, 0);
        let next = Batch { id: b3, start_ts: t0 + 2100, end_ts: t0 + 3000, status: "pending".into(), reason: None, attempts: 0 };
        write_idle_card(&st, &next, &stats).unwrap();
        assert!(live(&st).iter().any(|c| c.title == "Idle" && c.start_ts == t0 + 1150 && c.end_ts == t0 + 3000));
    }

    #[test]
    fn split_keeps_the_parts_outside_kept_spans() {
        let rc = |s: i64, e: i64| ResolvedCard { start_ts: s, end_ts: e, card: act("A", s, e), idle_meta: None };
        let out = split_around(vec![rc(0, 3600), rc(4000, 4030)], &[(600, 1200), (1200, 1800), (3570, 3700), (5000, 6000)]);
        let spans: Vec<(i64, i64, bool)> = out.iter().map(|(r, cut)| (r.start_ts, r.end_ts, *cut)).collect();
        // 3570..3600 is a 30 s sliver: dropped. An uncut short card stays.
        assert_eq!(spans, vec![(0, 600, true), (1800, 3570, true), (4000, 4030, false)]);
        assert_eq!(out[1].0.card.start_time, fmt_clock(1800));
        assert!(split_around(vec![rc(0, 600)], &[(0, 600)]).is_empty());
    }
}

/// The Claude Code provider end to end, with a shell script standing in for `claude` (no model
/// is called).
#[cfg(all(test, unix))]
mod cli_provider_tests {
    use super::*;
    use crate::config::{Paths, ProviderKind};

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let st = AppState::new(Paths::new(dir.path().to_path_buf()).unwrap()).unwrap();
        (dir, st)
    }

    /// A fake `claude` that records its arguments and answers with `reply` (or by `case` on the
    /// prompt, for scripts with several answers).
    fn fake_cli(dir: &std::path::Path, name: &str, body: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, format!("#!/bin/sh\necho \"$@\" >> \"{}.args\"\ninput=$(cat)\n{body}\n", path.display())).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.display().to_string()
    }

    fn envelope(result: &str) -> String {
        serde_json::json!({"type": "result", "subtype": "success", "is_error": false, "result": result,
            "usage": {"input_tokens": 100, "output_tokens": 50}})
        .to_string()
    }

    fn shots(st: &AppState, dir: &std::path::Path) -> i64 {
        let img = crate::imaging::encode_jpeg(&image::RgbImage::from_pixel(64, 36, image::Rgb([20, 90, 200])), 80).unwrap();
        let file = dir.join("frame.jpg");
        std::fs::write(&file, img).unwrap();
        // Start on a whole minute so the window is predictable.
        let base = (now_ts() - 2000) / 60 * 60;
        for i in 0..=91 {
            st.db.insert_screenshot(base + i * 10, &file.display().to_string(), 100, 0, Some("Code"), None, None, None).unwrap();
        }
        base
    }

    #[tokio::test]
    async fn one_step_timeline_through_the_claude_cli() {
        let (d, st) = state();
        let base = shots(&st, d.path());
        // Batch: base .. base+900; window: base .. base+960 (last minute + 1).
        let (ws, we) = (fmt_clock(base), fmt_clock(base + 960));
        let cards = serde_json::json!([{
            "startTime": ws, "endTime": we, "category": "work", "subcategory": "", "title": "Building the CLI provider",
            "summary": "Wrote the provider.", "detailedSummary": format!("{ws} - {}: edited one_step.rs in VS Code\n{} - {we}: ran cargo test", fmt_clock(base + 600), fmt_clock(base + 600)),
            "distractions": [], "appSites": {"primary": "code.visualstudio.com", "secondary": null}
        }])
        .to_string();
        let script = fake_cli(d.path(), "claude", &format!("cat <<'EOF'\n{}\nEOF", envelope(&cards)));
        {
            let mut s = st.settings.write().unwrap();
            s.provider = ProviderKind::ClaudeCli;
            s.base_url = script.clone();
            s.model = "haiku".into();
        }
        tick(&st).await.unwrap();
        let out = st.db.cards_in_range(0, i64::MAX, false).unwrap();
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].title, "Building the CLI provider");
        assert_eq!(out[0].category, "Work");
        assert_eq!((out[0].start_ts, out[0].end_ts), (base, base + 960));
        assert!(out[0].detailed_summary.contains("edited one_step.rs"), "model's detail lines kept");
        let obs = st.db.observations_in_range(0, i64::MAX).unwrap();
        assert_eq!(obs.len(), 2);
        assert_eq!(obs[0].text, "edited one_step.rs in VS Code");
        assert_eq!(st.db.batches_with_status(&["completed"], 0).unwrap().len(), 1);
        assert_eq!(st.db.token_usage_since(0).unwrap(), (100, 50));
        let args = std::fs::read_to_string(format!("{script}.args")).unwrap();
        assert!(args.contains("--session-id") && args.contains("--tools Read") && args.contains("--model haiku"), "{args}");
    }

    #[tokio::test]
    async fn signed_out_cli_hands_the_batch_to_the_fallback() {
        let (d, st) = state();
        let base = shots(&st, d.path());
        let signed_out = r#"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in · Please run /login"}"#;
        let main = fake_cli(d.path(), "claude-main", &format!("cat <<'EOF'\n{signed_out}\nEOF\nexit 1"));
        // The fallback is another Claude CLI install, used through the two-step Detailed path.
        let segments = serde_json::json!({"segments": [{"start": "00:00:00", "end": "00:15:00", "description": "VS Code: editing analysis.rs"}]}).to_string();
        let cards = serde_json::json!([{"startTime": fmt_clock(base), "endTime": fmt_clock(base + 900), "category": "Work", "subcategory": "",
            "title": "Editing the analysis loop", "summary": "Edited analysis.rs.", "distractions": [], "appSites": null}])
        .to_string();
        let backup = fake_cli(
            d.path(),
            "claude-backup",
            &format!("case \"$input\" in\n*\"Analyze these\"*) cat <<'EOF'\n{}\nEOF\n;;\n*) cat <<'EOF'\n{}\nEOF\n;;\nesac", envelope(&segments), envelope(&cards)),
        );
        {
            let mut s = st.settings.write().unwrap();
            s.provider = ProviderKind::ClaudeCli;
            s.base_url = main.clone();
            s.model = "haiku".into();
            s.fallback_provider = Some(ProviderKind::ClaudeCli);
            s.fallback_base_url = backup.clone();
            s.fallback_model = "sonnet".into();
        }
        tick(&st).await.unwrap();
        let out = st.db.cards_in_range(0, i64::MAX, false).unwrap();
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].title, "Editing the analysis loop");
        // The main CLI ran once (no two-step retry on a signed-out CLI); the backup twice.
        assert_eq!(std::fs::read_to_string(format!("{main}.args")).unwrap().lines().count(), 1);
        let backup_args = std::fs::read_to_string(format!("{backup}.args")).unwrap();
        assert_eq!(backup_args.lines().count(), 2);
        assert!(backup_args.contains("--model sonnet"));
        assert!(out[0].detailed_summary.contains("editing analysis.rs"), "Detailed mode builds the log from the transcription");
    }

    /// A one-step run that times out is not followed by the two-step path on the same CLI (two
    /// more runs of the timeout each, retried): the batch goes to the fallback provider.
    #[tokio::test]
    async fn timed_out_cli_hands_the_batch_to_the_fallback() {
        let (d, st) = state();
        let base = shots(&st, d.path());
        crate::llm::cli::TEST_TIMEOUT.with(|t| t.set(Some(std::time::Duration::from_millis(300))));
        let main = fake_cli(d.path(), "claude-slow", "exec sleep 5");
        let segments = serde_json::json!({"segments": [{"start": "00:00:00", "end": "00:15:00", "description": "VS Code: editing analysis.rs"}]}).to_string();
        let cards = serde_json::json!([{"startTime": fmt_clock(base), "endTime": fmt_clock(base + 900), "category": "Work", "subcategory": "",
            "title": "Editing the analysis loop", "summary": "Edited analysis.rs.", "distractions": [], "appSites": null}])
        .to_string();
        let backup = fake_cli(
            d.path(),
            "claude-backup",
            &format!("case \"$input\" in\n*\"Analyze these\"*) cat <<'EOF'\n{}\nEOF\n;;\n*) cat <<'EOF'\n{}\nEOF\n;;\nesac", envelope(&segments), envelope(&cards)),
        );
        {
            let mut s = st.settings.write().unwrap();
            s.provider = ProviderKind::ClaudeCli;
            s.base_url = main.clone();
            s.fallback_provider = Some(ProviderKind::ClaudeCli);
            s.fallback_base_url = backup.clone();
            s.fallback_model = "sonnet".into();
        }
        tick(&st).await.unwrap();
        crate::llm::cli::TEST_TIMEOUT.with(|t| t.set(None));
        let out = st.db.cards_in_range(0, i64::MAX, false).unwrap();
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].title, "Editing the analysis loop");
        assert_eq!(std::fs::read_to_string(format!("{main}.args")).unwrap().lines().count(), 1, "the slow CLI ran once");
        assert_eq!(std::fs::read_to_string(format!("{backup}.args")).unwrap().lines().count(), 2);
    }

    #[tokio::test]
    async fn timed_out_cli_without_fallback_fails_after_one_run() {
        let (d, st) = state();
        shots(&st, d.path());
        crate::llm::cli::TEST_TIMEOUT.with(|t| t.set(Some(std::time::Duration::from_millis(300))));
        let main = fake_cli(d.path(), "claude", "exec sleep 5");
        {
            let mut s = st.settings.write().unwrap();
            s.provider = ProviderKind::ClaudeCli;
            s.base_url = main.clone();
        }
        tick(&st).await.unwrap();
        crate::llm::cli::TEST_TIMEOUT.with(|t| t.set(None));
        let out = st.db.cards_in_range(0, i64::MAX, false).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, crate::db::ERROR_TITLE);
        assert!(out[0].detailed_summary.contains("timed out"), "{}", out[0].detailed_summary);
        assert_eq!(std::fs::read_to_string(format!("{main}.args")).unwrap().lines().count(), 1);
        // The catch-up of a later success may retry it; this run did not.
        assert_eq!(st.db.batches_with_status(&["failed"], 0).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn signed_out_cli_without_fallback_shows_how_to_sign_in() {
        let (d, st) = state();
        shots(&st, d.path());
        let signed_out = r#"{"type":"result","subtype":"success","is_error":true,"result":"Not logged in · Please run /login"}"#;
        let main = fake_cli(d.path(), "claude", &format!("cat <<'EOF'\n{signed_out}\nEOF\nexit 1"));
        {
            let mut s = st.settings.write().unwrap();
            s.provider = ProviderKind::ClaudeCli;
            s.base_url = main;
        }
        tick(&st).await.unwrap();
        let out = st.db.cards_in_range(0, i64::MAX, false).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].title, crate::db::ERROR_TITLE);
        assert!(out[0].summary.starts_with("Claude is signed out. Open a terminal (PowerShell), run `claude`"), "{}", out[0].summary);
    }
}
