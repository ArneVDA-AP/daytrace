//! Dayflow's two analysis pipelines.
//!
//! Local (small vision models): caption each sampled frame → group captions into 2–5 segments →
//! one card from summary + title → optionally merge with the previous card.
//!
//! Cloud (capable models): one request with up to 15 frames → segments → rewrite all cards in the
//! last 45 minutes, validated for coverage and the 10-minute minimum.

pub mod chat_api;
pub mod cli;
pub mod client;
pub mod failure;
pub mod one_step;
mod one_step_prompts;
pub mod prompts;
pub mod provider_cmds;
pub mod routing;

use crate::db::Shot;
use crate::timeutil::{clock_minutes, clock_span_minutes, fmt_clock, fmt_hhmmss, fmt_mmss, minutes_to_clock, parse_elapsed};
use crate::types::{normalize_category, ActivityCard, AppSites, Category, Observation};
use anyhow::{anyhow, bail, Result};
use base64::Engine;
use client::{parse_json, CallOpts, LlmClient, Part};
use prompts::CardText;
use serde::Deserialize;

pub struct Ctx<'a> {
    pub client: &'a LlmClient,
    pub batch_id: i64,
    pub categories: &'a [Category],
    pub language: &'a str,
    pub capture_interval: i64,
    /// User edits of the card prompt blocks.
    pub prompts: &'a crate::config::PromptOverrides,
}

/// A stored screenshot ready to send: frames already ≤720 px high are sent as stored; larger
/// ones (1080p setting) are downscaled and re-encoded at quality 85.
pub fn load_frame(path: &str) -> Result<Part> {
    let (w, h) = image::image_dimensions(path)?;
    let b64 = base64::engine::general_purpose::STANDARD;
    if h <= 720 {
        return Ok(Part::Jpeg { b64: b64.encode(std::fs::read(path)?), w, h });
    }
    let rgb = crate::imaging::downscale_rgb(image::open(path)?.to_rgb8(), 720);
    let (w, h) = rgb.dimensions();
    Ok(Part::Jpeg { b64: b64.encode(crate::imaging::encode_jpeg(&rgb, 85)?), w, h })
}

/// Text that stands in for a shot without an image.
fn hidden_note(shot: &Shot) -> Option<String> {
    if let Some(app) = &shot.hidden_app {
        return Some(format!("{app} window (hidden by privacy settings; not captured)"));
    }
    if shot.skip_reason.as_deref() == Some(crate::db::SKIP_IDLE) {
        return Some("screen not captured: no keyboard or mouse input (user away)".into());
    }
    None
}

// =============================================================================================
// Transcription
// =============================================================================================

#[derive(Deserialize)]
struct LocalSegments {
    segments: Vec<LocalSegment>,
}
#[derive(Deserialize, Clone)]
struct LocalSegment {
    #[serde(rename = "startTimestamp")]
    start: String,
    #[serde(rename = "endTimestamp")]
    end: String,
    description: String,
}

fn decode_local_segments(text: &str) -> Result<Vec<LocalSegment>> {
    if let Ok(s) = parse_json::<LocalSegments>(text) {
        return Ok(s.segments);
    }
    parse_json::<Vec<LocalSegment>>(text)
}

/// Local path transcription (Dayflow OllamaProvider+Transcription).
pub async fn transcribe_local(ctx: &Ctx<'_>, shots: &[Shot]) -> Result<Vec<Observation>> {
    let n = shots.len();
    if n == 0 {
        bail!("No screenshots to transcribe");
    }
    let stride = (n / 15).max(1);
    let sampled: Vec<&Shot> = shots.iter().step_by(stride).collect();
    let t0 = sampled[0].captured_at;

    let mut frames: Vec<(i64, String)> = Vec::new();
    let mut any_call_ok = false;
    for shot in &sampled {
        let offset = shot.captured_at - t0;
        if let Some(note) = hidden_note(shot) {
            frames.push((offset, note));
            continue;
        }
        let frame = match load_frame(&shot.file_path) {
            Ok(f) => f,
            Err(e) => {
                log::warn!("skipping unreadable frame {}: {e}", shot.file_path);
                continue;
            }
        };
        let res = ctx
            .client
            .complete(
                vec![Part::Text(prompts::DESCRIBE_FRAME.into()), frame],
                CallOpts { op: "describe_frame", batch_id: Some(ctx.batch_id), max_tokens: 4000, system: None, attempts: 1 },
            )
            .await;
        match res {
            Ok(text) => {
                any_call_ok = true;
                let t = text.trim().to_string();
                if !t.is_empty() {
                    frames.push((offset, t));
                }
            }
            Err(e) => {
                log::warn!("describe_frame failed at {offset}s: {e:#}");
                // A connection failure on every frame should fail fast rather than burn 15 calls.
                if !any_call_ok && e.downcast_ref::<reqwest::Error>().map(|r| r.is_connect()).unwrap_or(false) {
                    return Err(e.context("Could not reach the model server"));
                }
            }
        }
    }
    if frames.is_empty() {
        bail!("Failed to describe any screenshots");
    }

    let duration = sampled.last().unwrap().captured_at - t0;
    let duration_str = fmt_mmss(duration);
    let formatted: String = frames.iter().map(|(t, d)| format!("[{}] {}\n", fmt_mmss(*t), d)).collect();
    let base = prompts::segment_frames(frames.len(), &duration_str, &formatted);
    let mut prompt = base.clone();

    for attempt in 1..=2 {
        let outcome: Result<Vec<Observation>> = async {
            let text = ctx
                .client
                .complete(
                    vec![Part::Text(prompt.clone())],
                    CallOpts {
                        op: "segment_video_activity",
                        batch_id: Some(ctx.batch_id),
                        max_tokens: 4000,
                        system: Some(prompts::SYSTEM_JSON_LOCAL.into()),
                        attempts: 3,
                    },
                )
                .await?;
            let segments = decode_local_segments(&text)?;
            let dur = duration.max(1);
            let kept: Vec<(i64, i64, String)> = segments
                .into_iter()
                .map(|s| (parse_elapsed(&s.start), parse_elapsed(&s.end), s.description.trim().to_string()))
                .filter(|(a, b, _)| *a >= -30 && *b <= dur + 30)
                .collect();
            if kept.is_empty() {
                bail!("No valid segments in response");
            }
            if kept.len() > 5 {
                bail!("Too many segments ({}); expected 2-5", kept.len());
            }
            let covered: i64 = kept.iter().map(|(a, b, _)| (b - a).max(0)).sum();
            let coverage = covered as f64 / dur as f64;
            if coverage < 0.8 {
                return Err(anyhow::Error::msg(format!("coverage {coverage}")).context(CoverageMarker(coverage)));
            }
            let obs: Vec<Observation> = kept
                .into_iter()
                .filter_map(|(a, b, d)| {
                    let s = a.clamp(0, dur);
                    let e = b.clamp(s, dur);
                    (e > s).then(|| Observation { start_ts: t0 + s, end_ts: t0 + e, text: d })
                })
                .collect();
            if obs.is_empty() {
                bail!("No segments inside the recording");
            }
            Ok(obs)
        }
        .await;
        match outcome {
            Ok(obs) => return Ok(obs),
            Err(e) => {
                if attempt == 2 {
                    log::warn!("segment_video_activity exhausted ({e:#}); falling back to frame observations");
                    break;
                }
                prompt = match e.downcast_ref::<CoverageMarker>() {
                    Some(CoverageMarker(c)) => base.clone() + &prompts::segment_coverage_retry((c * 100.0).clamp(0.0, 100.0) as i64, &duration_str),
                    None => base.clone() + &prompts::segment_invalid_retry(&e.to_string()),
                };
            }
        }
    }
    // Fallback: one observation per described frame, running to the next frame.
    let mut obs = Vec::new();
    for (i, (t, d)) in frames.iter().enumerate() {
        let next = frames.get(i + 1).map(|(n, _)| *n).unwrap_or_else(|| (*t + ctx.capture_interval).max(duration));
        let end = if next > *t { next } else { t + ctx.capture_interval.max(1) };
        obs.push(Observation { start_ts: t0 + t, end_ts: t0 + end, text: d.clone() });
    }
    Ok(obs)
}

#[derive(Debug, Clone, Copy)]
struct CoverageMarker(f64);
impl std::fmt::Display for CoverageMarker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "segments covered only {:.0}% of the recording", self.0 * 100.0)
    }
}

#[derive(Deserialize)]
struct CloudSegments {
    segments: Vec<CloudSegment>,
}
#[derive(Deserialize, Clone)]
struct CloudSegment {
    start: String,
    end: String,
    #[serde(default)]
    description: String,
}

/// Dayflow's validateSegments with 2 s tolerance.
fn validate_cloud_segments(segs: &[CloudSegment], duration: i64) -> Option<String> {
    if segs.is_empty() {
        return Some("No segments returned.".into());
    }
    let tol = 2;
    let mut parsed: Vec<(i64, i64)> = Vec::new();
    for s in segs {
        let (a, b) = (parse_elapsed(&s.start), parse_elapsed(&s.end));
        if b <= a {
            return Some(format!("Segment end time must be after start time: {} -> {}", s.start, s.end));
        }
        if a < 0 {
            return Some(format!("Segment start time must be >= 00:00:00 (got {}).", s.start));
        }
        if duration > 0 && b > duration + tol {
            return Some(format!("Segment out of bounds: {} -> {} (duration {})", s.start, s.end, fmt_hhmmss(duration)));
        }
        parsed.push((a, b));
    }
    parsed.sort();
    if duration > 0 && parsed[0].0 > tol {
        return Some(format!("First segment must start at 00:00:00 (starts at {}).", fmt_hhmmss(parsed[0].0)));
    }
    for w in parsed.windows(2) {
        let gap = w[1].0 - w[0].1;
        if gap > tol {
            return Some(format!("Gap detected between segments: {} -> {}", fmt_hhmmss(w[0].1), fmt_hhmmss(w[1].0)));
        }
        if gap < -tol {
            return Some(format!("Overlap detected between segments: {} starts before {}", fmt_hhmmss(w[1].0), fmt_hhmmss(w[0].1)));
        }
    }
    let last_end = parsed.last().unwrap().1;
    if duration > 0 && duration - last_end > tol {
        return Some(format!("Last segment must end at {} (ends at {}).", fmt_hhmmss(duration), fmt_hhmmss(last_end)));
    }
    None
}

/// Run `f` up to 3 times, appending Dayflow's correction suffix after a failure.
async fn with_cloud_retries<T, F, Fut>(base_prompt: &str, mut f: F) -> Result<T>
where
    F: FnMut(String) -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut prompt = base_prompt.to_string();
    let mut last = anyhow!("no attempts");
    for attempt in 1..=3u64 {
        match f(prompt.clone()).await {
            Ok(v) => return Ok(v),
            Err(e) => {
                // Transport/auth errors are not fixable by re-prompting.
                if e.downcast_ref::<client::HttpError>().map(|h| h.status == 401 || h.status == 403 || h.status == 404).unwrap_or(false) {
                    return Err(e);
                }
                log::warn!("attempt {attempt} failed: {e:#}");
                prompt = base_prompt.to_string() + &prompts::cloud_retry_suffix(&format!("{e:#}"));
                last = e;
                if attempt < 3 {
                    tokio::time::sleep(std::time::Duration::from_secs(attempt)).await;
                }
            }
        }
    }
    Err(last)
}

/// Cloud path transcription (Dayflow OpenAICompatibleProvider.transcribeScreenshots).
pub async fn transcribe_cloud(ctx: &Ctx<'_>, shots: &[Shot]) -> Result<Vec<Observation>> {
    let (first, last) = match (shots.first(), shots.last()) {
        (Some(f), Some(l)) => (f.captured_at, l.captured_at),
        _ => bail!("No screenshots to transcribe"),
    };
    let duration = (last - first).max(1);

    // Up to 15 images, chosen evenly (first and last included) among shots that have one, and
    // fewer if the frames are so large that the request would approach the token ceiling.
    let with_image: Vec<&Shot> = shots.iter().filter(|s| s.has_image()).collect();
    let mut entries: Vec<(i64, Vec<Part>)> = Vec::new();
    let mut image_budget = client::MAX_INPUT_TOKENS - 10_000;
    let mut wanted = with_image.len().min(15);
    if let Some(first_img) = with_image.first() {
        if let Ok(Part::Jpeg { w, h, .. }) = load_frame(&first_img.file_path) {
            let per = crate::imaging::image_tokens(w, h).max(1);
            wanted = wanted.min(image_budget / per).max(1);
        }
    }
    let picks: Vec<&Shot> = (0..wanted)
        .map(|i| with_image[if wanted == 1 { 0 } else { i * (with_image.len() - 1) / (wanted - 1) }])
        .collect();
    for s in picks {
        if let Ok(frame) = load_frame(&s.file_path) {
            if let Part::Jpeg { w, h, .. } = &frame {
                let t = crate::imaging::image_tokens(*w, *h);
                if t > image_budget {
                    continue;
                }
                image_budget -= t;
            }
            entries.push((s.captured_at, vec![Part::Text(format!("Screenshot at {}:", fmt_hhmmss(s.captured_at - first))), frame]));
        }
    }
    // Stretches without images (away, private app) become one line each.
    let mut i = 0;
    while i < shots.len() {
        let Some(note) = hidden_note(&shots[i]) else {
            i += 1;
            continue;
        };
        let mut j = i;
        while j + 1 < shots.len() && hidden_note(&shots[j + 1]).as_deref() == Some(note.as_str()) {
            j += 1;
        }
        let (a, b) = (shots[i].captured_at - first, shots[j].captured_at - first);
        entries.push((shots[i].captured_at, vec![Part::Text(format!("From {} to {}: [{note}]", fmt_hhmmss(a), fmt_hhmmss(b)))]));
        i = j + 1;
    }
    entries.sort_by_key(|(t, _)| *t);
    let valid = entries.len();
    if valid == 0 {
        bail!("No readable screenshots to transcribe.");
    }
    let images: Vec<Part> = entries.into_iter().flat_map(|(_, p)| p).collect();
    let base = prompts::screenshot_transcription(
        valid,
        &fmt_hhmmss(duration),
        "00:00:00",
        &fmt_hhmmss(duration),
        "They are in chronological order, each labeled with its actual timestamp.",
    );
    with_cloud_retries(&base, |prompt| {
        let mut parts = vec![Part::Text(prompt)];
        parts.extend(images.iter().cloned());
        async move {
            let text = ctx
                .client
                .complete(
                    parts,
                    CallOpts {
                        op: "transcribe_screenshots",
                        batch_id: Some(ctx.batch_id),
                        max_tokens: 8000,
                        system: Some(prompts::SYSTEM_CLOUD.into()),
                        attempts: 2,
                    },
                )
                .await?;
            let mut resp: CloudSegments = parse_json(&text)?;
            // A zero-length segment (e.g. "00:14:59" -> "00:14:59" at the very end) carries no
            // time; drop it instead of rejecting the whole answer.
            resp.segments.retain(|s| parse_elapsed(&s.end) != parse_elapsed(&s.start));
            if let Some(err) = validate_cloud_segments(&resp.segments, duration) {
                bail!(err);
            }
            let mut segs = resp.segments;
            segs.sort_by_key(|s| parse_elapsed(&s.start));
            let mut out = Vec::new();
            for s in segs {
                let d = s.description.trim().to_string();
                if d.is_empty() {
                    bail!("Segment description is empty.");
                }
                let a = parse_elapsed(&s.start).max(0);
                let b = parse_elapsed(&s.end).min(duration);
                if b <= a {
                    bail!("Segment has no duration.");
                }
                out.push(Observation { start_ts: first + a, end_ts: first + b, text: d });
            }
            Ok(out)
        }
    })
    .await
}

// =============================================================================================
// Card generation
// =============================================================================================

fn obs_lines(obs: &[Observation]) -> Vec<String> {
    obs.iter().map(|o| format!("[{} - {}]: {}", fmt_clock(o.start_ts), fmt_clock(o.end_ts), o.text)).collect()
}

/// Case-insensitive removal of "The user"/"A user" (Dayflow's stripUserReferences).
fn strip_user_refs(s: &str) -> String {
    let mut out = s.to_string();
    for needle in ["the user", "a user"] {
        loop {
            let lower = out.to_ascii_lowercase(); // keeps byte offsets aligned with `out`
            match lower.find(needle) {
                Some(i) => out.replace_range(i..i + needle.len(), ""),
                None => break,
            }
        }
    }
    out
}

#[derive(Deserialize)]
struct SummaryResp {
    #[serde(default)]
    summary: String,
    #[serde(default)]
    category: String,
    #[serde(default)]
    app_sites: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct MergeDecision {
    combine: bool,
}

#[derive(Deserialize)]
struct Merged {
    title: String,
    summary: String,
}

async fn local_json_call<T: serde::de::DeserializeOwned>(
    ctx: &Ctx<'_>,
    op: &'static str,
    base: &str,
    retry: fn(&str) -> String,
) -> Result<T> {
    let mut prompt = base.to_string();
    let mut last = anyhow!("no attempts");
    for attempt in 1..=3 {
        let res: Result<T> = async {
            let text = ctx
                .client
                .complete(
                    vec![Part::Text(prompt.clone())],
                    CallOpts { op, batch_id: Some(ctx.batch_id), max_tokens: 4000, system: Some(prompts::SYSTEM_JSON_LOCAL.into()), attempts: 3 },
                )
                .await?;
            parse_json::<T>(&text)
        }
        .await;
        match res {
            Ok(v) => return Ok(v),
            Err(e) => {
                if attempt < 3 {
                    prompt = base.to_string() + &retry(&e.to_string());
                }
                last = e;
            }
        }
    }
    Err(last)
}

fn normalize_title(raw: &str) -> String {
    let line = raw.trim().lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
    let line = line.strip_prefix("Title:").or_else(|| line.strip_prefix("title:")).unwrap_or(line).trim();
    line.trim_matches(|c| c == '"' || c == '\'' || c == '“' || c == '”').trim().to_string()
}

/// Local path cards (Dayflow OllamaProvider.generateActivityCards): uses only this batch's
/// observations, builds one card, and possibly merges it with the last existing card.
pub async fn cards_local(ctx: &Ctx<'_>, batch_obs: &[Observation], existing: &[ActivityCard]) -> Result<Vec<ActivityCard>> {
    if batch_obs.is_empty() {
        bail!("No observations");
    }
    let observations_text = strip_user_refs(&obs_lines(batch_obs).join("\n\n"));
    let summary: SummaryResp = local_json_call(
        ctx,
        "generate_summary",
        &prompts::local_summary(&observations_text, ctx.categories, ctx.language, ctx.prompts),
        prompts::local_summary_retry,
    )
    .await?;

    let bullets: String = batch_obs
        .iter()
        .map(|o| o.text.trim())
        .filter(|t| !t.is_empty())
        .map(|t| format!("- {t}"))
        .collect::<Vec<_>>()
        .join("\n");
    let title_base = prompts::local_title(&bullets, ctx.language, ctx.prompts);
    let mut prompt = title_base.clone();
    let mut title = String::new();
    let mut last_err = None;
    for attempt in 1..=3 {
        match ctx
            .client
            .complete(
                vec![Part::Text(prompt.clone())],
                CallOpts { op: "generate_title", batch_id: Some(ctx.batch_id), max_tokens: 4000, system: Some(prompts::SYSTEM_TEXT_LOCAL.into()), attempts: 3 },
            )
            .await
        {
            Ok(t) => {
                title = normalize_title(&t);
                if !title.is_empty() {
                    break;
                }
                last_err = Some(anyhow!("Empty title response"));
            }
            Err(e) => last_err = Some(e),
        }
        if attempt < 3 {
            prompt = title_base.clone() + &prompts::local_title_retry(&last_err.as_ref().map(|e| e.to_string()).unwrap_or_default());
        }
    }
    if title.is_empty() {
        return Err(last_err.unwrap_or_else(|| anyhow!("Empty title response")));
    }

    let new_card = ActivityCard {
        start_time: fmt_clock(batch_obs.first().unwrap().start_ts),
        end_time: fmt_clock(batch_obs.iter().map(|o| o.end_ts).max().unwrap()),
        category: normalize_category(&summary.category, ctx.categories),
        subcategory: String::new(),
        title,
        summary: summary.summary.trim().to_string(),
        detailed_summary: String::new(),
        distractions: None,
        app_sites: summary.app_sites.as_ref().and_then(crate::types::app_sites_from_value),
    };

    let mut all: Vec<ActivityCard> = existing.to_vec();
    let Some(last) = all.last().cloned() else {
        all.push(new_card);
        return Ok(all);
    };
    let dur_last = clock_span_minutes(&last.start_time, &last.end_time).unwrap_or(0.0);
    let gap = clock_span_minutes(&last.end_time, &new_card.start_time).unwrap_or(0.0);
    let combined = clock_span_minutes(&last.start_time, &new_card.end_time).unwrap_or(0.0);
    if dur_last >= 40.0 || gap > 5.0 || combined > 60.0 {
        all.push(new_card);
        return Ok(all);
    }
    let prev_t = CardText { start: &last.start_time, end: &last.end_time, title: &last.title, summary: &last.summary };
    let new_t = CardText { start: &new_card.start_time, end: &new_card.end_time, title: &new_card.title, summary: &new_card.summary };
    let decision: MergeDecision =
        local_json_call(ctx, "evaluate_card_merge", &prompts::merge_decision(&prev_t, &new_t), prompts::merge_decision_retry).await?;
    if !decision.combine {
        all.push(new_card);
        return Ok(all);
    }
    let merged: Merged =
        local_json_call(ctx, "merge_cards", &prompts::merge_cards(&prev_t, &new_t, ctx.language), prompts::merge_cards_retry).await?;
    let merged_card = ActivityCard {
        start_time: last.start_time.clone(),
        end_time: new_card.end_time.clone(),
        category: last.category.clone(),
        subcategory: last.subcategory.clone(),
        title: normalize_title(&merged.title),
        summary: merged.summary.trim().to_string(),
        detailed_summary: last.detailed_summary.clone(),
        distractions: last.distractions.clone(),
        app_sites: last.app_sites.clone().or(new_card.app_sites.clone()),
    };
    if clock_span_minutes(&merged_card.start_time, &merged_card.end_time).unwrap_or(0.0) > 60.0 {
        all.push(new_card);
    } else {
        *all.last_mut().unwrap() = merged_card;
    }
    Ok(all)
}

/// Dayflow's validateTimeCoverage: every existing card's span must be covered by the output
/// (3-minute flexibility).
pub fn validate_coverage(existing: &[ActivityCard], new: &[ActivityCard]) -> Option<String> {
    if existing.is_empty() {
        return None;
    }
    // Put every card on one continuous minute axis around the first input card. Dayflow maps each
    // card to minutes-of-day independently, so a card starting after midnight (12:13 AM = 13) is
    // compared against one that started before it (11:13 PM = 1393) as if it were 23 hours
    // earlier, and valid output fails coverage.
    let base = clock_minutes(&existing[0].start_time);
    let range = |c: &ActivityCard| {
        let mut s = clock_minutes(&c.start_time);
        while s >= base + 720.0 {
            s -= 1440.0;
        }
        while s < base - 720.0 {
            s += 1440.0;
        }
        (s, s + clock_span_minutes(&c.start_time, &c.end_time).unwrap_or(0.0))
    };
    let mut input: Vec<(f64, f64)> = existing.iter().map(range).collect();
    input.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let mut merged: Vec<(f64, f64)> = Vec::new();
    for r in input {
        match merged.last_mut() {
            Some(l) if r.0 <= l.1 + 1.0 => l.1 = l.1.max(r.1),
            _ => merged.push(r),
        }
    }
    let output: Vec<(f64, f64)> = new.iter().map(range).filter(|(s, e)| e - s >= 0.1).collect();
    let flex = 3.0;
    let mut uncovered: Vec<(f64, f64)> = Vec::new();
    for (istart, iend) in merged {
        let mut cur = istart;
        let mut guard = 10000;
        while cur < iend && guard > 0 {
            guard -= 1;
            if let Some(o) = output.iter().find(|o| o.0 - flex <= cur && cur <= o.1 + flex) {
                cur = (cur + 0.01).max(o.1);
                continue;
            }
            let next = output.iter().filter(|o| o.0 > cur && o.0 < iend).map(|o| o.0).fold(iend, f64::min);
            if next > cur {
                uncovered.push((cur, next.min(iend)));
                cur = next;
            } else {
                uncovered.push((cur, iend));
                break;
            }
        }
    }
    let missing: Vec<String> = uncovered
        .iter()
        .filter(|(s, e)| e - s > flex)
        .map(|(s, e)| format!("{}-{} ({} min)", minutes_to_clock(*s), minutes_to_clock(*e), (e - s) as i64))
        .collect();
    if missing.is_empty() {
        return None;
    }
    let mut msg = format!("Missing coverage for time segments: {}", missing.join(", "));
    msg += "\n\nINPUT CARDS:";
    for (i, c) in existing.iter().enumerate() {
        msg += &format!("\n  {}. {} - {}: {}", i + 1, c.start_time, c.end_time, c.title);
    }
    msg += "\n\nOUTPUT CARDS:";
    for (i, c) in new.iter().enumerate() {
        msg += &format!("\n  {}. {} - {}: {}", i + 1, c.start_time, c.end_time, c.title);
    }
    Some(msg)
}

/// Every card except the last must be at least 10 minutes.
pub fn validate_durations(cards: &[ActivityCard]) -> Option<String> {
    for (i, c) in cards.iter().enumerate() {
        let d = clock_span_minutes(&c.start_time, &c.end_time).unwrap_or(0.0);
        if d < 10.0 && i < cards.len() - 1 {
            return Some(format!("Card {} '{}' is only {:.1} minutes long", i + 1, c.title, d));
        }
    }
    None
}

/// Cloud path cards: rewrite the whole 45-minute window from all window observations.
pub async fn cards_cloud(ctx: &Ctx<'_>, window_obs: &[Observation], existing: &[ActivityCard]) -> Result<Vec<ActivityCard>> {
    // Previous cards go in without detailedSummary: the model does not write it any more.
    let slim: Vec<serde_json::Value> = existing
        .iter()
        .map(|c| {
            let mut v = serde_json::to_value(c).unwrap_or_default();
            if let Some(o) = v.as_object_mut() {
                o.remove("detailedSummary");
            }
            v
        })
        .collect();
    let previous = serde_json::to_string_pretty(&slim)?;
    let transcript = obs_lines(window_obs).join("\n");
    let base = prompts::cloud_cards(&previous, &transcript, ctx.categories, ctx.language, ctx.prompts);
    with_cloud_retries(&base, |prompt| async move {
        let text = ctx
            .client
            .complete(
                vec![Part::Text(prompt)],
                CallOpts { op: "generate_cards", batch_id: Some(ctx.batch_id), max_tokens: 16000, system: Some(prompts::SYSTEM_CLOUD.into()), attempts: 2 },
            )
            .await?;
        let decoded: Vec<ActivityCard> = parse_json(&text)?;
        if decoded.is_empty() {
            bail!("No cards returned.");
        }
        let cards: Vec<ActivityCard> = decoded
            .into_iter()
            .map(|mut c| {
                c.category = normalize_category(&c.category, ctx.categories);
                c
            })
            .collect();
        let errs: Vec<String> = [validate_coverage(existing, &cards), validate_durations(&cards)].into_iter().flatten().collect();
        if !errs.is_empty() {
            bail!(errs.join("\n"));
        }
        Ok(cards)
    })
    .await
}

/// Fill each card's step-by-step log from the transcription. Each observation goes to the one
/// card that contains its midpoint, as "start – end text", with the clip-relative times the
/// transcription sometimes mentions ("at 00:10:35") removed.
pub fn assign_detailed(cards: &mut [crate::db::ResolvedCard], obs: &[Observation]) {
    static CLIP_TIME: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = CLIP_TIME.get_or_init(|| {
        regex::Regex::new(r"(?i)\s*\(?\b(?:at|by|from|to|until|through|around|after|before)?\s*(?:about\s+)?0\d:[0-5]\d(?::[0-5]\d)?\b\)?").unwrap()
    });
    for card in cards.iter_mut() {
        let lines: Vec<String> = obs
            .iter()
            .filter(|o| {
                let mid = (o.start_ts + o.end_ts) / 2;
                mid >= card.start_ts && mid < card.end_ts
            })
            .map(|o| {
                let text = re.replace_all(o.text.trim(), "").trim().to_string();
                format!("{} – {} {}", fmt_clock(o.start_ts.max(card.start_ts)), fmt_clock(o.end_ts.min(card.end_ts)), text)
            })
            .collect();
        card.card.detailed_summary = lines.join("\n");
    }
}

#[allow(dead_code)]
fn _assert_app_sites(_: AppSites) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(s: &str, e: &str) -> ActivityCard {
        ActivityCard {
            start_time: s.into(),
            end_time: e.into(),
            category: "Work".into(),
            subcategory: String::new(),
            title: "t".into(),
            summary: String::new(),
            detailed_summary: String::new(),
            distractions: None,
            app_sites: None,
        }
    }

    #[test]
    fn coverage_validation() {
        let existing = vec![card("2:00 PM", "2:30 PM")];
        assert!(validate_coverage(&existing, &[card("2:00 PM", "2:45 PM")]).is_none());
        assert!(validate_coverage(&existing, &[card("2:02 PM", "2:28 PM")]).is_none()); // within flex
        let err = validate_coverage(&existing, &[card("2:00 PM", "2:10 PM"), card("2:20 PM", "2:45 PM")]);
        assert!(err.unwrap().contains("Missing coverage")); // gap minus the 3-min flex
    }

    #[test]
    fn detailed_log_by_midpoint() {
        use crate::db::ResolvedCard;
        let mk = |s, e| ResolvedCard { start_ts: s, end_ts: e, card: card("1:00 PM", "1:10 PM"), idle_meta: None };
        let mut cards = vec![mk(0, 600), mk(600, 1200)];
        let obs = vec![
            Observation { start_ts: 0, end_ts: 500, text: "VS Code: editing, changes apps at about 00:10:35".into() },
            Observation { start_ts: 500, end_ts: 900, text: "Browser: docs".into() }, // midpoint 700 -> card 2
        ];
        assign_detailed(&mut cards, &obs);
        assert_eq!(cards[0].card.detailed_summary.lines().count(), 1);
        assert!(!cards[0].card.detailed_summary.contains("00:10:35"), "{}", cards[0].card.detailed_summary);
        assert!(cards[1].card.detailed_summary.contains("Browser: docs"));
    }

    #[test]
    fn coverage_across_midnight() {
        // From a real run: valid output was rejected because 12:13 AM was compared as if it
        // came 23 hours before 11:13 PM.
        let existing = vec![card("11:13 PM", "12:43 AM"), card("12:43 AM", "12:58 AM")];
        let out = vec![card("11:13 PM", "12:13 AM"), card("12:13 AM", "12:43 AM"), card("12:43 AM", "12:58 AM"), card("12:58 AM", "1:14 AM")];
        assert_eq!(validate_coverage(&existing, &out), None);
        // A real gap after midnight is still caught.
        let gap = vec![card("11:13 PM", "12:13 AM"), card("12:30 AM", "1:14 AM")];
        assert!(validate_coverage(&existing, &gap).unwrap().contains("12:16 AM-12:30 AM")); // gap minus the 3-min flex
    }

    #[test]
    fn duration_validation() {
        assert!(validate_durations(&[card("2:00 PM", "2:10 PM"), card("2:10 PM", "2:13 PM")]).is_none());
        assert!(validate_durations(&[card("2:00 PM", "2:05 PM"), card("2:05 PM", "2:30 PM")]).is_some());
    }

    #[test]
    fn segment_validation() {
        let s = |a: &str, b: &str| CloudSegment { start: a.into(), end: b.into(), description: "x".into() };
        assert!(validate_cloud_segments(&[s("00:00:00", "00:05:00"), s("00:05:01", "00:15:00")], 900).is_none());
        assert!(validate_cloud_segments(&[s("00:00:00", "00:05:00"), s("00:06:00", "00:15:00")], 900).unwrap().contains("Gap"));
        assert!(validate_cloud_segments(&[s("00:00:00", "00:10:00")], 900).unwrap().contains("Last segment"));
    }

    #[test]
    fn strips_user_refs_and_titles() {
        assert_eq!(strip_user_refs("The user opened X. a USER typed"), " opened X.  typed");
        assert_eq!(normalize_title("Title: \"Fixing Cedar sign-in\"\nextra"), "Fixing Cedar sign-in");
    }
}

#[cfg(test)]
mod regression {
    use super::*;
    #[test]
    fn strip_user_refs_handles_non_ascii() {
        assert_eq!(strip_user_refs("İzmir trip: the user’s calendar"), "İzmir trip: ’s calendar");
        assert_eq!(strip_user_refs("ẞ the user éé"), "ẞ  éé");
    }
}
