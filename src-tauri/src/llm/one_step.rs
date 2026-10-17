//! Dayflow's one-step timeline for the CLI providers (OneStepTimelineSupport.swift,
//! ClaudeProvider+Timeline.swift, CodexProvider+Timeline.swift, ClaudeOutputValidator.swift):
//! the batch's screenshots go to the CLI as image files together with the previous cards, and
//! the CLI returns the rewritten cards in one call, with no transcript in between. Observations
//! are rebuilt from the cards' detail lines so recaps and chat keep their per-batch record.
//!
//! Daytrace differences: previous cards are all non-idle cards in the 45-minute window (as in
//! Daytrace's Detailed mode) rather than only the cards connected to the batch; times without a
//! screenshot (away, private app) are listed in the prompt; the user's name is not sent. Every
//! run is checked against the 90k input-token ceiling (`client::MAX_INPUT_TOKENS`) like the HTTP
//! providers: fewer frames are sent when they would not fit, and a correction that would push a
//! resumed session past the ceiling is not sent.

use super::cli::args::{Profile, Session};
use super::cli::output::Usage;
use super::cli::{run_timeout, CliBackend, CliRequest, CliTool};
use super::client::{estimate_tokens, Part, MAX_INPUT_TOKENS};
use super::one_step_prompts as p;
use super::Ctx;
use crate::db::{Shot, StoredCard};
use crate::timeutil::{fmt_clock, local, resolve_card_range};
use crate::types::{normalize_category, ActivityCard, Observation};
use anyhow::{anyhow, bail, Result};
use serde_json::Value;
use std::time::Instant;

/// Dayflow ClaudeTranscriptionInputBuilder.maximumFrameCount.
pub const MAX_FRAMES: usize = 15;
const MAX_ATTEMPTS: usize = 3;
/// Room under the ceiling for what the CLI adds itself (its own instructions and the Read tool)
/// and for the two correction turns of a resumed Claude session.
pub const CLI_RESERVE_TOKENS: usize = 14_000;
/// Dayflow ClaudeTimelineTolerance.
const SOURCE_CONNECTION_SECS: i64 = 60;
const OUTPUT_COVERAGE_SECS: i64 = 59;

pub struct OneStepResult {
    pub cards: Vec<ActivityCard>,
    pub observations: Vec<Observation>,
    pub model: String,
    /// The new window the cards were validated against.
    pub window: (i64, i64),
}

/// Up to `max` indices spread evenly over `count` items, first and last included.
pub fn evenly_spaced(count: usize, max: usize) -> Vec<usize> {
    if count == 0 || max == 0 {
        return Vec::new();
    }
    if count <= max || max == 1 {
        return (0..count.min(max)).collect();
    }
    (0..max).map(|i| ((i as f64) * (count - 1) as f64 / (max - 1) as f64).round() as usize).collect()
}

/// The frames to send so that `text_tokens` plus the frames stay within `limit`: as many as fit,
/// spread evenly (first and last kept). Empty when not even one fits.
pub fn frames_within(frame_tokens: &[usize], text_tokens: usize, limit: usize) -> Vec<usize> {
    (1..=frame_tokens.len())
        .rev()
        .map(|keep| evenly_spaced(frame_tokens.len(), keep))
        .find(|idx| text_tokens + idx.iter().map(|&i| frame_tokens[i]).sum::<usize>() <= limit)
        .unwrap_or_default()
}

/// Input estimate of a prompt with its system prompt (images counted separately).
fn text_tokens(prompt: &str, system: &str) -> usize {
    estimate_tokens(&[Part::Text(prompt.to_string())], (!system.is_empty()).then_some(system))
}

fn over_ceiling(estimate: usize) -> anyhow::Error {
    anyhow!("Request would be about {estimate} input tokens, over the {MAX_INPUT_TOKENS} limit; not sent")
}

/// The new window: the first shot's minute to one minute after the last shot's minute.
pub fn window_bounds(first_ts: i64, last_ts: i64) -> (i64, i64) {
    (first_ts - first_ts.rem_euclid(60), last_ts - last_ts.rem_euclid(60) + 60)
}

/// True when the frames are about a minute apart and no two share a minute label.
pub fn is_one_frame_per_minute(times: &[i64]) -> bool {
    if times.len() <= 1 {
        return true;
    }
    let mut gaps: Vec<i64> = times.windows(2).map(|w| w[1] - w[0]).collect();
    gaps.sort();
    let median = gaps[gaps.len() / 2];
    let minutes: std::collections::HashSet<i64> = times.iter().map(|t| t.div_euclid(60)).collect();
    (45..=90).contains(&median) && minutes.len() == times.len()
}

fn frame_label(ts: i64, per_minute: bool) -> String {
    if per_minute { fmt_clock(ts) } else { local(ts).format("%-I:%M:%S %p").to_string() }
}

/// Inputs of one prompt.
pub struct PromptInput<'a> {
    pub tool: CliTool,
    /// (capture time, file path) per frame, in order.
    pub frames: &'a [(i64, String)],
    pub window: (i64, i64),
    pub previous: &'a [ActivityCard],
    pub categories: &'a [crate::types::Category],
    pub language: &'a str,
    /// Lines for times without a screenshot.
    pub not_captured: &'a [String],
}

/// Dayflow buildTimelinePrompt (Claude and Codex variants).
pub fn build_prompt(i: &PromptInput) -> String {
    let times: Vec<i64> = i.frames.iter().map(|f| f.0).collect();
    let per_minute = is_one_frame_per_minute(&times);
    let mut prompt = p::TEMPLATE
        .replace(p::CATEGORIES_PLACEHOLDER, &super::prompts::categories_section(i.categories))
        .replace(p::LANGUAGE_PLACEHOLDER, &super::prompts::language_instruction(i.language, true).unwrap_or_default())
        .replace(p::NAME_GIVEN, p::NAME_MISSING);
    if !per_minute {
        prompt = prompt.replace(p::PER_MINUTE_FRAMES, p::IRREGULAR_FRAMES);
    }
    let previous = serde_json::to_string(i.previous).unwrap_or_else(|_| "[]".into());
    let (ws, we) = (fmt_clock(i.window.0), fmt_clock(i.window.1));
    let mut text = String::new();
    match i.tool {
        CliTool::Claude => {
            text += prompt.trim();
            text += "\n\nINPUTS:\n";
        }
        CliTool::Codex => {
            text += p::CODEX_SYSTEM;
            text += "\n\n";
            text += prompt.trim();
            text += "\n";
            text += p::CODEX_ADDITIONAL_RULES;
            text += "\n\nINPUTS:\n";
        }
    }
    // Concatenation, not format!, keeps screen-derived text in previous cards from being read as
    // format syntax.
    text += "Previous cards: ";
    text += &previous;
    text += "\n";
    let lines: Vec<String> = match i.tool {
        CliTool::Claude => {
            text += &format!("New screenshots: {} frames, covering {ws} to {we}. Each is a local image file:\n", i.frames.len());
            i.frames.iter().map(|(t, path)| format!("Frame at {}: {path}", frame_label(*t, per_minute))).collect()
        }
        CliTool::Codex => {
            text += &format!("New screenshots: {} frames are attached to this message as images, in order, covering {ws} to {we}:\n", i.frames.len());
            i.frames.iter().enumerate().map(|(n, (t, _))| format!("Image {}: frame at {}", n + 1, frame_label(*t, per_minute))).collect()
        }
    };
    text += &lines.join("\n");
    text += "\n\n";
    if !i.not_captured.is_empty() {
        text += "Times without a screenshot:\n";
        text += &i.not_captured.join("\n");
        text += "\n\n";
    }
    match i.tool {
        CliTool::Claude => {
            text += "Tool requirement: use Read exactly once on every listed file before answering. Do not narrate or summarize individual frames before the answer.\n\n";
            text += p::OUTPUT_BLOCK;
        }
        CliTool::Codex => {
            text += "Do not run commands or use tools; everything you need is in this message.\n\n";
            text += p::CODEX_OUTPUT_BLOCK;
        }
    }
    text
}

/// Dayflow buildTimelineCorrectionPrompt (Claude, same session).
pub fn claude_correction(err: &str, window: (i64, i64)) -> String {
    let (ws, we) = (fmt_clock(window.0), fmt_clock(window.1));
    format!(
        "The previous JSON output has validation errors. Fix it using the screenshots and context already in this conversation.\n\nIssues:\n{err}\n\nRequirements:\n- Return the FULL corrected JSON array, not a diff.\n- The cards together must cover the previous cards' time and the new window, {ws} to {we}, with no gaps and no overlaps, and must not extend past {we}.\n- Every card except the last must be 10 to 60 minutes long. Fold a shorter stretch into the neighbouring card and keep its detail lines.\n- Keep the detailedSummary lines that were correct; only change what the issues require.\n- Do not reread files or use tools. Output JSON only, with no code fences or extra text."
    )
}

/// Codex correction: each run is a fresh session, so it rides on the original prompt.
pub fn codex_correction(base: &str, err: &str, window: (i64, i64)) -> String {
    let (ws, we) = (fmt_clock(window.0), fmt_clock(window.1));
    format!(
        "{base}\n\nPREVIOUS ATTEMPT FAILED - FIX THE FOLLOWING:\n{err}\n\nThe cards together must cover the previous cards' time and the new window, {ws} to {we}, with no gaps and no overlaps. Every card except the last must be 10 to 60 minutes long. Return JSON only."
    )
}

/// Dayflow CodexProvider.availabilityFallback: a newer model the account cannot use yet.
pub fn codex_model_fallback(model: &str, error: &str) -> Option<&'static str> {
    let text = error.to_lowercase();
    if ["invalid_api_key", "insufficient_quota", "usage limit", "country, region", "failed to refresh token"].iter().any(|s| text.contains(s)) {
        return None;
    }
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"model_not_found|unsupported_model|unknown model|model not found|\bmodel\b[^\r\n]{0,160}(?:does not exist|not available|not supported|(?:do not|don't|not) have access)").unwrap()
    });
    if !re.is_match(&text) {
        return None;
    }
    match model {
        "gpt-6.1-sol" => Some("gpt-5.6-sol"),
        "gpt-6-luna" => Some("gpt-5.6-luna"),
        _ => None,
    }
}

fn strip_fences(s: &str) -> String {
    s.replace("```json", "").replace("```JSON", "").replace("```", "").trim().to_string()
}

fn card_from_value(v: &Value) -> Option<ActivityCard> {
    let mut o = v.as_object()?.clone();
    for (alias, key) in [("start", "startTime"), ("end", "endTime")] {
        if !o.contains_key(key) {
            if let Some(x) = o.get(alias).cloned() {
                o.insert(key.into(), x);
            }
        }
    }
    let mut card: ActivityCard = serde_json::from_value(Value::Object(o)).ok()?;
    if card.detailed_summary.trim().is_empty() {
        card.detailed_summary = card.summary.clone();
    }
    Some(card)
}

fn cards_from_value(v: &Value) -> Option<Vec<ActivityCard>> {
    let items = match v {
        Value::Array(a) => a,
        Value::Object(o) => o.get("cards")?.as_array()?,
        _ => return None,
    };
    items.iter().map(card_from_value).collect()
}

/// The `[` that opens the array ending at the last `]` (Dayflow findBalancedArrayStart).
fn last_balanced_array(s: &str) -> Option<&str> {
    let end = s.rfind(']')?;
    let mut balance = 0i32;
    for (i, c) in s[..=end].char_indices().rev() {
        match c {
            ']' => balance += 1,
            '[' => {
                balance -= 1;
                if balance == 0 {
                    return Some(&s[i..=end]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Dayflow AgentCLISupport.parseCards: `{"cards":[...]}`, a bare array, or the last balanced
/// array inside other text.
pub fn parse_cards(output: &str, stderr: &str) -> Result<Vec<ActivityCard>> {
    let cleaned = strip_fences(output);
    if let Ok(v) = serde_json::from_str::<Value>(&cleaned) {
        if let Some(c) = cards_from_value(&v) {
            return Ok(c);
        }
    }
    if let Some(slice) = last_balanced_array(&cleaned) {
        if let Some(c) = serde_json::from_str::<Value>(slice).ok().and_then(|v| cards_from_value(&v)) {
            return Ok(c);
        }
    }
    if let (Some(a), Some(b)) = (cleaned.find('{'), cleaned.rfind('}')) {
        if b > a {
            if let Some(c) = serde_json::from_str::<Value>(&cleaned[a..=b]).ok().and_then(|v| cards_from_value(&v)) {
                return Ok(c);
            }
        }
    }
    if let Some(e) = super::cli::output::extract_cli_error(output, stderr) {
        bail!(e);
    }
    bail!("Failed to decode activity cards")
}

#[derive(Clone, Copy)]
struct Span {
    start: i64,
    end: i64,
}

fn merged(mut spans: Vec<Span>, tolerance: i64) -> Vec<Span> {
    spans.retain(|s| s.end > s.start);
    spans.sort_by_key(|s| (s.start, s.end));
    let mut out: Vec<Span> = Vec::new();
    for s in spans {
        match out.last_mut() {
            Some(l) if s.start <= l.end + tolerance => l.end = l.end.max(s.end),
            _ => out.push(s),
        }
    }
    out
}

fn is_covered(target: Span, by: &[Span], tolerance: i64) -> bool {
    let mut through = target.start;
    for c in by {
        if c.end <= through || c.start >= target.end {
            continue;
        }
        if c.start > through + tolerance {
            return false;
        }
        through = through.max(c.end);
        if through >= target.end - tolerance {
            return true;
        }
    }
    through >= target.end - tolerance
}

/// Dayflow ClaudeOutputValidator.validateActivityCards, on Unix times: card lengths (10–60 min,
/// the last card may be shorter), no overlaps, no card across a real gap in the evidence, nothing
/// outside the evidence, and every piece of evidence (previous cards and the new window) covered.
pub fn validate_timeline(cards: &[ActivityCard], previous: &[(i64, i64)], window: (i64, i64)) -> std::result::Result<(), String> {
    if cards.is_empty() {
        return Err("No activity cards were returned.".into());
    }
    let anchor = window.0;
    let mut spans = Vec::new();
    for c in cards {
        let (s, e) = resolve_card_range(&c.start_time, &c.end_time, anchor).ok_or_else(|| format!("Invalid timestamp: {} - {}", c.start_time, c.end_time))?;
        spans.push(Span { start: s, end: e });
    }
    for (i, (c, s)) in cards.iter().zip(&spans).enumerate() {
        let d = s.end - s.start;
        let last = i == cards.len() - 1;
        if d <= 0 || d > 3600 || (!last && d < 600) {
            return Err(format!("Card '{}' has an invalid duration of {d} seconds.", c.title));
        }
        if i > 0 && s.start < spans[i - 1].end {
            return Err(format!("Activity cards overlap: '{}' and '{}'.", cards[i - 1].title, c.title));
        }
    }
    let mut evidence: Vec<Span> = previous.iter().map(|(s, e)| Span { start: *s, end: *e }).collect();
    evidence.push(Span { start: window.0, end: window.1 });
    let evidence = merged(evidence, SOURCE_CONNECTION_SECS);
    for w in evidence.windows(2) {
        let (gap_start, gap_end) = (w[0].end, w[1].start);
        if gap_end - gap_start > SOURCE_CONNECTION_SECS {
            if let Some((c, _)) = cards.iter().zip(&spans).find(|(_, s)| s.start < gap_start && s.end > gap_end) {
                return Err(format!("Card '{}' covers a genuine gap in the source timeline.", c.title));
            }
        }
    }
    for s in &spans {
        if !is_covered(*s, &evidence, OUTPUT_COVERAGE_SECS) {
            return Err("Activity cards contain material output outside the supplied source timeline.".into());
        }
    }
    let mut sorted = spans.clone();
    sorted.sort_by_key(|s| (s.start, s.end));
    for e in &evidence {
        if !is_covered(*e, &sorted, OUTPUT_COVERAGE_SECS) {
            return Err("Activity cards do not cover all supplied observations and previous cards.".into());
        }
    }
    Ok(())
}

/// Dayflow OneStepTimelineObservations.parseTimelineDetailLine.
pub fn parse_detail_line(line: &str) -> Option<(String, String, String)> {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"^\s*[-•*]?\s*(\d{1,2}:\d{2}\s*[APap][Mm])\s*[-–—]\s*(\d{1,2}:\d{2}\s*[APap][Mm])\s*:?\s*(.+)$").unwrap()
    });
    let c = re.captures(line)?;
    Some((c[1].to_string(), c[2].to_string(), c[3].to_string()))
}

fn clock_near(clock: &str, anchor: i64) -> Option<i64> {
    resolve_card_range(clock, clock, anchor).map(|(s, _)| s)
}

/// Dayflow OneStepTimelineObservations.make: one observation per detail line inside the new
/// window, or one per card when no line can be read.
pub fn observations_from_cards(cards: &[ActivityCard], window: (i64, i64)) -> Vec<Observation> {
    let (ws, we) = window;
    let clipped = |start: i64, end: i64, text: &str| -> Option<Observation> {
        let t = text.trim();
        let end = if end <= start { start + 60 } else { end };
        if t.is_empty() || start >= we || end <= ws {
            return None;
        }
        let s = start.max(ws);
        Some(Observation { start_ts: s, end_ts: end.min(we).max(s + 1), text: t.to_string() })
    };
    let mut out: Vec<Observation> = cards
        .iter()
        .flat_map(|c| c.detailed_summary.lines().map(str::to_string).collect::<Vec<_>>())
        .filter_map(|line| {
            let (a, b, text) = parse_detail_line(&line)?;
            clipped(clock_near(&a, ws)?, clock_near(&b, ws)?, &text)
        })
        .collect();
    if out.is_empty() {
        out = cards
            .iter()
            .filter_map(|c| clipped(clock_near(&c.start_time, ws)?, clock_near(&c.end_time, ws)?, &format!("{}: {}", c.title, c.summary)))
            .collect();
    }
    out.sort_by_key(|o| (o.start_ts, o.end_ts));
    out
}

/// One-step generation through the context's CLI provider. `previous` are the stored cards the
/// model rewrites.
pub async fn generate_timeline(ctx: &Ctx<'_>, shots: &[Shot], previous: &[StoredCard]) -> Result<OneStepResult> {
    let cli = ctx.client.cli().ok_or_else(|| anyhow!("One-step generation needs a CLI provider"))?;
    let (Some(first), Some(last)) = (shots.first(), shots.last()) else { bail!("No screenshots to generate a timeline from") };
    let window = window_bounds(first.captured_at, last.captured_at);

    let folder = cli.temp_dir("frames")?;
    let with_image: Vec<&Shot> = shots.iter().filter(|s| s.has_image()).collect();
    // (capture time, file, estimated input tokens) per frame.
    let mut written: Vec<(i64, String, usize)> = Vec::new();
    for idx in evenly_spaced(with_image.len(), MAX_FRAMES) {
        let s = with_image[idx];
        let file = folder.path().join(format!("frame_{:02}.jpg", written.len() + 1));
        match write_frame(&s.file_path, &file) {
            Ok((w, h)) => written.push((s.captured_at, file.display().to_string(), crate::imaging::image_tokens(w, h))),
            Err(e) => log::warn!("skipping unreadable timeline frame {}: {e:#}", s.file_path),
        }
    }
    if written.is_empty() {
        bail!("No screenshots could be loaded for timeline generation.");
    }
    let not_captured = not_captured_lines(shots);
    let previous_cards: Vec<ActivityCard> = previous.iter().map(|c| c.to_activity()).collect();
    let previous_spans: Vec<(i64, i64)> = previous.iter().map(|c| (c.start_ts, c.end_ts)).collect();
    let prompt_for = |frames: &[(i64, String)]| {
        build_prompt(&PromptInput {
            tool: cli.tool,
            frames,
            window,
            previous: &previous_cards,
            categories: ctx.categories,
            language: ctx.language,
            not_captured: &not_captured,
        })
    };
    let system = match cli.tool {
        CliTool::Claude => p::CLAUDE_SYSTEM,
        CliTool::Codex => "",
    };
    // The ceiling: the prompt with every frame listed (an upper bound for fewer), then as many
    // frames as fit next to it.
    let all: Vec<(i64, String)> = written.iter().map(|(t, f, _)| (*t, f.clone())).collect();
    let text = text_tokens(&prompt_for(&all), system);
    let tokens: Vec<usize> = written.iter().map(|w| w.2).collect();
    let keep = frames_within(&tokens, text, MAX_INPUT_TOKENS - CLI_RESERVE_TOKENS);
    if keep.is_empty() {
        return Err(over_ceiling(text + tokens.iter().min().copied().unwrap_or(0) + CLI_RESERVE_TOKENS));
    }
    if keep.len() < written.len() {
        log::info!("one-step: sending {} of {} frames to stay under the {MAX_INPUT_TOKENS}-token ceiling", keep.len(), written.len());
        for (i, w) in written.iter().enumerate() {
            if !keep.contains(&i) {
                let _ = std::fs::remove_file(&w.1);
            }
        }
    }
    let frames: Vec<(i64, String)> = keep.iter().map(|&i| all[i].clone()).collect();
    let image_tokens: usize = keep.iter().map(|&i| tokens[i]).sum();
    let base = prompt_for(&frames);
    let budget = Budget { image_tokens, system };
    match cli.tool {
        CliTool::Claude => run_claude(ctx, cli, &base, folder.path(), window, &previous_spans, budget).await,
        CliTool::Codex => {
            let images: Vec<String> = frames.iter().map(|f| f.1.clone()).collect();
            run_codex(ctx, cli, &base, &images, window, &previous_spans, budget).await
        }
    }
}

/// What a run's requests weigh besides their prompt text.
#[derive(Clone, Copy)]
struct Budget {
    image_tokens: usize,
    system: &'static str,
}

impl Budget {
    /// The estimate of a request with `prompt` and the frames.
    fn request(&self, prompt: &str) -> usize {
        text_tokens(prompt, self.system) + self.image_tokens
    }
}

/// Copies or downscales a frame to at most 1080 px high; returns the written size.
fn write_frame(src: &str, dst: &std::path::Path) -> Result<(u32, u32)> {
    let (w, h) = image::image_dimensions(src)?;
    if h <= 1080 {
        std::fs::copy(src, dst)?;
        return Ok((w, h));
    }
    let rgb = crate::imaging::downscale_rgb(image::open(src)?.to_rgb8(), 1080);
    let size = (rgb.width(), rgb.height());
    std::fs::write(dst, crate::imaging::encode_jpeg(&rgb, 85)?)?;
    Ok(size)
}

fn not_captured_lines(shots: &[Shot]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < shots.len() {
        let Some(note) = super::hidden_note(&shots[i]) else {
            i += 1;
            continue;
        };
        let mut j = i;
        while j + 1 < shots.len() && super::hidden_note(&shots[j + 1]).as_deref() == Some(note.as_str()) {
            j += 1;
        }
        out.push(format!("- From {} to {}: {note}", fmt_clock(shots[i].captured_at), fmt_clock(shots[j].captured_at)));
        i = j + 1;
    }
    out
}

fn validated(ctx: &Ctx<'_>, text: &str, stderr: &str, window: (i64, i64), previous: &[(i64, i64)]) -> Result<Vec<ActivityCard>> {
    let cards: Vec<ActivityCard> = parse_cards(text, stderr)?
        .into_iter()
        .map(|mut c| {
            c.category = normalize_category(&c.category, ctx.categories);
            c
        })
        .collect();
    validate_timeline(&cards, previous, window).map_err(|e| anyhow!(e))?;
    Ok(cards)
}

fn log_call(ctx: &Ctx<'_>, model: &str, started: Instant, prompt: &str, result: std::result::Result<&str, String>, usage: Option<Usage>) {
    let Some(db) = ctx.client.call_log() else { return };
    let latency = started.elapsed().as_millis() as i64;
    let usage = usage.map(|u| u.for_log()).unwrap_or((None, None));
    match result {
        Ok(out) => db.log_llm_call(Some(ctx.batch_id), "generate_timeline", model, latency, prompt, Some(out), None, usage),
        Err(e) => db.log_llm_call(Some(ctx.batch_id), "generate_timeline", model, latency, prompt, None, Some(&e), usage),
    }
}

async fn run_claude(ctx: &Ctx<'_>, cli: &CliBackend, base: &str, frames_dir: &std::path::Path, window: (i64, i64), previous: &[(i64, i64)], budget: Budget) -> Result<OneStepResult> {
    let session = super::cli::new_uuid();
    // Removes the resumable transcript however this ends (Dayflow ClaudeSessionCleanup).
    struct Cleanup<'a>(&'a CliBackend, String);
    impl Drop for Cleanup<'_> {
        fn drop(&mut self) {
            self.0.cleanup_session(&self.1);
        }
    }
    // A marker for the next start, in case Quit or a crash skips the cleanup below.
    cli.begin_session(&session);
    let _cleanup = Cleanup(cli, session.clone());
    let mut prompt = base.to_string();
    let mut last_err = anyhow!("No timeline cards returned.");
    // A resumed session carries everything before it: the first request, each answer and each
    // correction.
    let mut context = budget.request(base);
    for attempt in 1..=MAX_ATTEMPTS {
        if context > MAX_INPUT_TOKENS {
            if attempt == 1 {
                return Err(over_ceiling(context));
            }
            log::warn!("generate_timeline: correction not sent; the resumed session would be about {context} input tokens");
            break;
        }
        let req = CliRequest {
            prompt: prompt.clone(),
            profile: Profile::Timeline,
            session: if attempt == 1 { Session::Start(session.clone()) } else { Session::Resume(session.clone()) },
            system: p::CLAUDE_SYSTEM.into(),
            effort: None,
            read_dir: Some(frames_dir.to_path_buf()),
            images: Vec::new(),
            timeout: run_timeout(),
            model: None,
        };
        let started = Instant::now();
        // A failed run (signed out, limit, timeout) ends the attempt loop, as in Dayflow.
        let reply = match cli.run(&req).await {
            Ok(r) => r,
            Err(e) => {
                let usage = e.downcast_ref::<super::cli::CliFailed>().and_then(|f| f.usage);
                log_call(ctx, &cli.model, started, &prompt, Err(format!("{e:#}")), usage);
                return Err(e);
            }
        };
        match validated(ctx, &reply.text, &reply.stderr, window, previous) {
            Ok(cards) => {
                log_call(ctx, &cli.model, started, &prompt, Ok(&reply.text), reply.usage);
                let observations = observations_from_cards(&cards, window);
                return Ok(OneStepResult { cards, observations, model: cli.model.clone(), window });
            }
            Err(e) => {
                log_call(ctx, &cli.model, started, &prompt, Err(format!("{e:#}")), reply.usage);
                log::warn!("generate_timeline attempt {attempt} failed validation: {e:#}");
                prompt = claude_correction(&format!("{e:#}"), window);
                context += text_tokens(&reply.text, "") + text_tokens(&prompt, "");
                last_err = e;
            }
        }
    }
    Err(last_err)
}

async fn run_codex(ctx: &Ctx<'_>, cli: &CliBackend, base: &str, images: &[String], window: (i64, i64), previous: &[(i64, i64)], budget: Budget) -> Result<OneStepResult> {
    let mut model = cli.model.clone();
    let mut prompt = base.to_string();
    let mut last_err = anyhow!("No timeline cards returned.");
    for attempt in 1..=MAX_ATTEMPTS {
        // Each run is a new session: the prompt (with any correction) and the images.
        let estimate = budget.request(&prompt);
        if estimate > MAX_INPUT_TOKENS {
            if attempt == 1 {
                return Err(over_ceiling(estimate));
            }
            log::warn!("generate_timeline (codex): correction not sent; it would be about {estimate} input tokens");
            break;
        }
        let req = CliRequest {
            prompt: prompt.clone(),
            profile: Profile::Text,
            session: Session::Ephemeral,
            system: String::new(),
            effort: Some("high".into()),
            read_dir: None,
            images: images.to_vec(),
            timeout: run_timeout(),
            model: Some(model.clone()),
        };
        let started = Instant::now();
        match cli.run(&req).await {
            Ok(reply) => match validated(ctx, &reply.text, &reply.stderr, window, previous) {
                Ok(cards) => {
                    log_call(ctx, &model, started, &prompt, Ok(&reply.text), None);
                    let observations = observations_from_cards(&cards, window);
                    return Ok(OneStepResult { cards, observations, model, window });
                }
                Err(e) => {
                    log_call(ctx, &model, started, &prompt, Err(format!("{e:#}")), None);
                    prompt = codex_correction(base, &format!("{e:#}"), window);
                    last_err = e;
                }
            },
            Err(e) => {
                let msg = format!("{e:#}");
                log_call(ctx, &model, started, &prompt, Err(msg.clone()), None);
                let kind = super::failure::classify(&msg).kind;
                if kind.is_actionable() && kind != super::failure::FailureKind::ModelMisconfigured {
                    return Err(e);
                }
                if let Some(next) = codex_model_fallback(&model, &msg) {
                    log::info!("Codex model {model} is not available; trying {next}");
                    model = next.to_string();
                } else if kind == super::failure::FailureKind::ModelMisconfigured {
                    return Err(e);
                }
                prompt = base.to_string();
                last_err = e;
            }
        }
        log::warn!("generate_timeline (codex) attempt {attempt} failed: {last_err:#}");
    }
    Err(last_err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timeutil::resolve_card_range;
    use chrono::{Local, TimeZone};

    fn ts(h: u32, m: u32) -> i64 {
        Local.with_ymd_and_hms(2026, 3, 10, h, m, 0).single().unwrap().timestamp()
    }

    fn card(s: &str, e: &str, title: &str) -> ActivityCard {
        ActivityCard {
            start_time: s.into(),
            end_time: e.into(),
            category: "Work".into(),
            subcategory: String::new(),
            title: title.into(),
            summary: "S".into(),
            detailed_summary: String::new(),
            distractions: None,
            app_sites: None,
        }
    }

    #[test]
    fn frame_selection_and_window() {
        assert_eq!(evenly_spaced(90, 15).len(), 15);
        assert_eq!(evenly_spaced(90, 15)[0], 0);
        assert_eq!(*evenly_spaced(90, 15).last().unwrap(), 89);
        assert_eq!(evenly_spaced(4, 15), vec![0, 1, 2, 3]);
        // More than half of the items: still spread to the last one (card screenshots).
        assert_eq!(evenly_spaced(10, 6), vec![0, 2, 4, 5, 7, 9]);
        let (a, b) = window_bounds(ts(9, 0) + 12, ts(9, 14) + 50);
        assert_eq!((a, b), (ts(9, 0), ts(9, 15)));
        let minute: Vec<i64> = (0..15).map(|i| ts(9, 0) + i * 64).collect();
        assert!(is_one_frame_per_minute(&minute));
        let tight: Vec<i64> = (0..15).map(|i| ts(9, 0) + i * 10).collect();
        assert!(!is_one_frame_per_minute(&tight));
    }

    #[test]
    fn frames_fit_under_the_ceiling() {
        // 15 ultrawide frames at 1080 px (3840x1080: 138 x 39 patches) do not all fit.
        let per = crate::imaging::image_tokens(3840, 1080);
        let tokens = vec![per; MAX_FRAMES];
        let limit = MAX_INPUT_TOKENS - CLI_RESERVE_TOKENS;
        let keep = frames_within(&tokens, 6_000, limit);
        assert!(keep.len() < MAX_FRAMES && keep.len() >= 10, "{}", keep.len());
        assert!(6_000 + keep.len() * per <= limit);
        assert!(6_000 + (keep.len() + 1) * per > limit, "as many as fit");
        assert_eq!((keep[0], *keep.last().unwrap()), (0, MAX_FRAMES - 1), "spread over the batch");
        // 1080p frames fit as they are.
        let hd = vec![crate::imaging::image_tokens(1920, 1080); MAX_FRAMES];
        assert_eq!(frames_within(&hd, 6_000, limit).len(), MAX_FRAMES);
        // A prompt that is too long on its own: nothing is sent.
        assert!(frames_within(&hd, limit, limit).is_empty());
        assert!(over_ceiling(95_000).to_string().contains("over the 90000 limit; not sent"));
    }

    #[test]
    fn prompt_has_inputs_and_verbatim_blocks() {
        let frames = vec![(ts(9, 0), r"C:\t\frames-1\frame_01.jpg".to_string()), (ts(9, 1), r"C:\t\frames-1\frame_02.jpg".to_string())];
        let prev = vec![card("8:40 AM", "9:00 AM", "Fixing {braces} in \"titles\"")];
        let cats = crate::types::categories_for_llm(&crate::types::default_categories());
        let input = PromptInput {
            tool: CliTool::Claude,
            frames: &frames,
            window: (ts(9, 0), ts(9, 2)),
            previous: &prev,
            categories: &cats,
            language: "Dutch",
            not_captured: &["- From 9:01 AM to 9:01 AM: Bitwarden window (hidden by privacy settings; not captured)".to_string()],
        };
        let text = build_prompt(&input);
        assert!(text.starts_with("You are turning screenshots of a user's screen into timeline cards."));
        assert!(text.contains("USER CATEGORIES (choose exactly one label):"));
        assert!(text.contains("The user only speaks Dutch."));
        assert!(!text.contains("{{"));
        assert!(text.contains(p::NAME_MISSING) && !text.contains(p::NAME_GIVEN));
        assert!(text.contains(p::PER_MINUTE_FRAMES));
        assert!(text.contains("Previous cards: [{\"startTime\":\"8:40 AM\""));
        assert!(text.contains("Fixing {braces} in \\\"titles\\\""));
        assert!(text.contains("New screenshots: 2 frames, covering 9:00 AM to 9:02 AM. Each is a local image file:\nFrame at 9:00 AM: C:\\t\\frames-1\\frame_01.jpg\nFrame at 9:01 AM: "));
        assert!(text.contains("Times without a screenshot:\n- From 9:01 AM"));
        assert!(text.trim_end().ends_with(p::OUTPUT_BLOCK.trim_end()));

        let codex = build_prompt(&PromptInput { tool: CliTool::Codex, ..input });
        assert!(codex.starts_with(p::CODEX_SYSTEM));
        assert!(codex.contains("Image 2: frame at 9:01 AM"));
        assert!(!codex.contains("frame_01.jpg"));
        assert!(codex.contains("\"frames\": ["));
    }

    #[test]
    fn irregular_frames_use_seconds() {
        let frames: Vec<(i64, String)> = (0..5).map(|i| (ts(9, 0) + i * 10, format!("f{i}"))).collect();
        let cats = crate::types::default_categories();
        let text = build_prompt(&PromptInput { tool: CliTool::Claude, frames: &frames, window: (ts(9, 0), ts(9, 1)), previous: &[], categories: &cats, language: "", not_captured: &[] });
        assert!(text.contains(p::IRREGULAR_FRAMES));
        assert!(text.contains("Frame at 9:00:10 AM: f1"));
    }

    #[test]
    fn parses_card_shapes() {
        let arr = r#"Here you go: [{"startTime":"9:00 AM","endTime":"9:15 AM","category":"Work","subcategory":"","title":"T","summary":"S","detailedSummary":"9:00 AM - 9:15 AM: edited [draft] in Notion","distractions":[],"appSites":{"primary":"notion.so","secondary":""}}]"#;
        let c = parse_cards(arr, "").unwrap();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].app_sites.as_ref().unwrap().primary.as_deref(), Some("notion.so"));
        let env = r#"```json
{"frames":[{"time":"9:00 AM","app":"Code","note":"n"}],"cards":[{"start":"9:00 AM","end":"9:15 AM","category":"Work","subcategory":"","title":"T","summary":"Only summary"}]}
```"#;
        let c = parse_cards(env, "").unwrap();
        assert_eq!(c[0].start_time, "9:00 AM");
        assert_eq!(c[0].detailed_summary, "Only summary");
        let e = parse_cards("I could not read the files.", "ERROR: You've hit your usage limit.").unwrap_err();
        assert!(e.to_string().contains("usage limit"));
        assert_eq!(parse_cards("nothing", "").unwrap_err().to_string(), "Failed to decode activity cards");
    }

    #[test]
    fn validation_follows_dayflow_rules() {
        let window = (ts(9, 0), ts(9, 15));
        let prev = [(ts(8, 40), ts(9, 0))];
        // Extends the previous card through the window: valid.
        assert_eq!(validate_timeline(&[card("8:40 AM", "9:15 AM", "A")], &prev, window), Ok(()));
        // Two cards, the last one short: valid.
        assert_eq!(validate_timeline(&[card("8:40 AM", "9:08 AM", "A"), card("9:08 AM", "9:15 AM", "B")], &prev, window), Ok(()));
        // A short card in the middle.
        let e = validate_timeline(&[card("8:40 AM", "8:45 AM", "A"), card("8:45 AM", "9:15 AM", "B")], &prev, window).unwrap_err();
        assert!(e.contains("invalid duration"), "{e}");
        // Drops the previous card's time.
        let e = validate_timeline(&[card("9:00 AM", "9:15 AM", "A")], &prev, window).unwrap_err();
        assert!(e.contains("do not cover all"), "{e}");
        // Runs past the window.
        let e = validate_timeline(&[card("8:40 AM", "9:30 AM", "A")], &prev, window).unwrap_err();
        assert!(e.contains("outside the supplied source"), "{e}");
        // Overlap.
        let e = validate_timeline(&[card("8:40 AM", "9:00 AM", "A"), card("8:55 AM", "9:15 AM", "B")], &prev, window).unwrap_err();
        assert!(e.contains("overlap"), "{e}");
        // A real gap between the previous card and the window must stay a gap.
        let gap_prev = [(ts(8, 20), ts(8, 40))];
        let e = validate_timeline(&[card("8:20 AM", "9:15 AM", "A")], &gap_prev, window).unwrap_err();
        assert!(e.contains("genuine gap"), "{e}");
        assert_eq!(validate_timeline(&[card("8:20 AM", "8:40 AM", "A"), card("9:00 AM", "9:15 AM", "B")], &gap_prev, window), Ok(()));
        // Nothing returned / unreadable time.
        assert!(validate_timeline(&[], &prev, window).is_err());
        assert!(validate_timeline(&[card("soon", "later", "A")], &prev, window).unwrap_err().contains("Invalid timestamp"));
    }

    #[test]
    fn observations_from_detail_lines() {
        let window = (ts(9, 0), ts(9, 15));
        let mut a = card("8:40 AM", "9:15 AM", "A");
        a.detailed_summary = "8:40 AM - 8:59 AM: earlier work\n9:00 AM - 9:07 AM: edited \"Q4 plan\" in Notion\n- 9:07 AM – 9:07 AM: replied in Slack\nnot a line".into();
        let obs = observations_from_cards(&[a.clone()], window);
        assert_eq!(obs.len(), 2);
        assert_eq!((obs[0].start_ts, obs[0].end_ts), (ts(9, 0), ts(9, 7)));
        assert_eq!(obs[0].text, "edited \"Q4 plan\" in Notion");
        // A zero-length line counts as one minute.
        assert_eq!((obs[1].start_ts, obs[1].end_ts), (ts(9, 7), ts(9, 8)));
        // Without readable lines: one observation per card, clipped to the window.
        a.detailed_summary = "no times here".into();
        let obs = observations_from_cards(&[a], window);
        assert_eq!(obs.len(), 1);
        assert_eq!((obs[0].start_ts, obs[0].end_ts), window);
        assert_eq!(obs[0].text, "A: S");
        let _ = resolve_card_range("9:00 AM", "9:15 AM", window.0);
    }

    #[test]
    fn codex_model_fallbacks() {
        assert_eq!(codex_model_fallback("gpt-6-luna", "ERROR: The model `gpt-6-luna` does not exist or you do not have access to it."), Some("gpt-5.6-luna"));
        assert_eq!(codex_model_fallback("gpt-6-luna", "ERROR: You've hit your usage limit"), None);
        assert_eq!(codex_model_fallback("o5", "model_not_found"), None);
    }
}
