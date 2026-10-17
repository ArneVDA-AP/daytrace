//! Weekly review without a model call. Ported from Dayflow (MIT, (c) 2025 Jerry Liu):
//! Core/Weekly/WeeklyDateRange.swift (weeks start Monday at 4 AM), WeeklyDonutBuilder.swift
//! (distribution, top four plus Other), WeeklyDashboardBuilder.swift (app names),
//! WeeklyDashboardBuilder+HeatmapWorkflow.swift (weekly workflow grid and the focus heat map),
//! WeeklyDashboardBuilder+Interactions.swift (context shifts and distractions per day) and
//! WeeklyDashboardBuilder+TreemapSankey.swift (apps per category, change against the week before).
//!
//! Totals reuse the day statistics, so the Daily and Weekly views agree.

use super::stats::{self, Block, CategoryTotal, Kind, NamedSecs, Seg};
use crate::db::StoredCard;
use crate::types::Category;
use chrono::{Datelike, Duration, NaiveDate};
use serde::Serialize;

const WORKFLOW_SLOT: f64 = 15.0;
const HEAT_BUCKET: f64 = 5.0;
const OTHER_COLOR: &str = "#BFB6AE";
const DISTRACTION_COLOR: &str = "#FF5950";

/// Cards of one logical day (overlapping its bounds; clipped here).
pub struct DayInput {
    pub day: String,
    pub start: i64,
    pub end: i64,
    pub cards: Vec<StoredCard>,
}

/// Monday to Sunday of the week that contains `day` (a logical day, YYYY-MM-DD).
pub fn week_days(day: &str) -> Option<Vec<String>> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    let monday = date - Duration::days(date.weekday().num_days_from_monday() as i64);
    Some((0..7).map(|i| (monday + Duration::days(i)).format("%Y-%m-%d").to_string()).collect())
}

fn weekday_label(day: &str, long: bool) -> String {
    NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .map(|d| d.format(if long { "%A" } else { "%a" }).to_string())
        .unwrap_or_default()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeekDaySummary {
    pub day: String,
    pub label: String,
    pub captured_secs: i64,
    pub focus_secs: i64,
    pub distracted_secs: i64,
    pub idle_secs: i64,
    pub totals: Vec<CategoryTotal>,
    pub context_shifts: u32,
    pub distraction_count: u32,
    pub longest_focus: Option<Block>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeekCell {
    pub key: String,
    pub name: String,
    pub color: String,
    pub occupancy: f64,
    pub secs: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeekGridRow {
    pub day: String,
    pub label: String,
    pub cells: Vec<Option<WeekCell>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeekGrid {
    pub start_minute: f64,
    pub end_minute: f64,
    pub slot_minutes: f64,
    pub rows: Vec<WeekGridRow>,
    pub totals: Vec<CategoryTotal>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeatRow {
    pub day: String,
    pub label: String,
    /// -1 (focused) to 1 (distracted) per bucket; 0 is no signal.
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Heatmap {
    pub start_minute: f64,
    pub end_minute: f64,
    pub bucket_minutes: f64,
    pub rows: Vec<HeatRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HourRow {
    pub day: String,
    pub label: String,
    /// Distracted seconds per clock hour, starting at 4 AM.
    pub secs: Vec<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WeekBlock {
    pub day: String,
    pub start_ts: i64,
    pub end_ts: i64,
    pub secs: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppUsage {
    pub name: String,
    pub secs: i64,
    /// Same app (and category, in `apps_by_category`) the week before; 0 when unused.
    pub previous_secs: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppCategory {
    pub key: String,
    pub name: String,
    pub color: String,
    pub secs: i64,
    pub apps: Vec<AppUsage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WeekStats {
    pub week_start: String,
    pub week_end: String,
    pub days: Vec<WeekDaySummary>,
    /// Weekly distribution (no Idle or System): top four categories plus Other.
    pub distribution: Vec<CategoryTotal>,
    pub captured_secs: i64,
    pub focus_secs: i64,
    pub distracted_secs: i64,
    pub idle_secs: i64,
    pub workflow: WeekGrid,
    pub heatmap: Heatmap,
    pub distraction_hours: Vec<HourRow>,
    pub context_insight: String,
    pub focus_blocks: Vec<WeekBlock>,
    pub apps_by_category: Vec<AppCategory>,
    pub top_apps: Vec<AppUsage>,
    /// Foreground programs from screenshot metadata; filled by the command.
    pub programs: Vec<NamedSecs>,
}

// ---- app names (WeeklyDashboardBuilder.prettyAppName) ----

const APP_NAMES: &[(&str, &str)] = &[
    ("chatgpt", "ChatGPT"),
    ("claude", "Claude"),
    ("codex", "Codex"),
    ("cursor", "Cursor"),
    ("xcode", "Xcode"),
    ("dayflow", "Dayflow"),
    ("figma", "Figma"),
    ("slack", "Slack"),
    ("zoom", "Zoom"),
    ("meet.google", "Meet"),
    ("google meet", "Meet"),
    ("youtube", "YouTube"),
    ("reddit", "Reddit"),
    ("twitter", "X"),
    ("x.com", "X"),
    ("substack", "Substack"),
    ("notion", "Notion"),
    ("linear", "Linear"),
    ("github", "GitHub"),
    ("safari", "Safari"),
    ("chrome", "Chrome"),
    ("calendar", "Calendar"),
    ("mail", "Mail"),
    ("messages", "Messages"),
    ("maps", "Maps"),
    ("clickup", "ClickUp"),
    ("runway", "Runway"),
    ("flora", "Flora"),
    // Windows apps (not in Dayflow's list)
    ("visual studio code", "VS Code"),
    ("vs code", "VS Code"),
    ("vscode", "VS Code"),
    ("microsoft edge", "Edge"),
    ("firefox", "Firefox"),
    ("microsoft teams", "Teams"),
    ("outlook", "Outlook"),
    ("file explorer", "File Explorer"),
    ("windows terminal", "Terminal"),
    ("powershell", "PowerShell"),
];

fn title_case(s: &str) -> String {
    s.split(' ')
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Display name for an app or site string from a card's appSites.
pub fn pretty_app_name(raw: &str) -> String {
    let source = raw.trim();
    let lower = source.to_lowercase();
    if let Some((_, name)) = APP_NAMES.iter().find(|(needle, _)| lower.contains(needle)) {
        return name.to_string();
    }
    let first = source.split([',', ';', '|', '\n']).find(|p| !p.is_empty()).unwrap_or(source);
    let cleaned = first.replace("https://", "").replace("http://", "").replace("www.", "").replace("com.apple.", "");
    let cleaned = cleaned.trim();
    if cleaned.contains('.') && !cleaned.contains(' ') {
        if let Some(part) = cleaned.split('.').find(|p| !p.is_empty()) {
            return title_case(part);
        }
    }
    title_case(if cleaned.is_empty() { "Other" } else { cleaned })
}

fn key_of(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("_")
}

/// (key, name) of the card's primary app or site. Cards without appSites have none (Dayflow
/// guesses from the title and summary; that attributed time to apps merely mentioned).
pub fn app_of(card: &StoredCard) -> Option<(String, String)> {
    let sites = card.app_sites.as_ref()?;
    let raw = [sites.primary.as_deref(), sites.secondary.as_deref()]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|s| !s.is_empty())?;
    let name = pretty_app_name(raw);
    let key = key_of(&name);
    (!key.is_empty() && key != "other").then_some((key, name))
}

// ---- heat map (WeeklyDashboardBuilder+HeatmapWorkflow.swift) ----

fn text_says_distraction(text: &str) -> bool {
    let t = text.to_lowercase();
    t.contains("distraction") || t.contains("distracted")
}

/// isFullDistractionFact: the whole card counts as distraction.
fn is_full_distraction(s: &Seg) -> bool {
    if s.cat.kind == Kind::Distraction || text_says_distraction(&format!("{} {}", s.cat.name, s.card.subcategory)) {
        return true;
    }
    if !s.card.distractions.is_empty() {
        return false;
    }
    text_says_distraction(&format!("{} {} {} {}", s.cat.name, s.card.subcategory, s.card.title, s.card.summary))
}

fn add_interval(values: &mut [f64], start: f64, end: f64, vs: f64, bucket: f64, weight: f64) {
    let ve = vs + values.len() as f64 * bucket;
    let (s, e) = (start.max(vs), end.min(ve));
    if e <= s {
        return;
    }
    let first = ((s - vs) / bucket).floor().max(0.0) as usize;
    let last = ((((e - vs) / bucket).ceil() as i64) - 1).min(values.len() as i64 - 1);
    if last < first as i64 {
        return;
    }
    for i in first..=last as usize {
        let bs = vs + i as f64 * bucket;
        let o = (e.min(bs + bucket) - s.max(bs)).max(0.0);
        values[i] += o * weight;
    }
}

fn run_lengths(clean: &[bool]) -> Vec<usize> {
    let mut out = vec![0; clean.len()];
    let mut i = 0;
    while i < clean.len() {
        if !clean[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < clean.len() && clean[i] {
            i += 1;
        }
        for v in out.iter_mut().take(i).skip(start) {
            *v = i - start;
        }
    }
    out
}

/// Dayflow's heatmapScores plus smoothFocusedHeatmapScores.
pub fn heat_scores(focus: &[f64], distraction: &[f64], switches: &[f64], bucket: f64) -> Vec<f64> {
    let clean: Vec<bool> = (0..focus.len()).map(|i| focus[i] >= bucket * 0.6 && distraction[i] < 1.0).collect();
    let runs = run_lengths(&clean);
    let raw: Vec<f64> = (0..focus.len())
        .map(|i| {
            let focus_ratio = (focus[i] / bucket).clamp(0.0, 1.0);
            let distraction_ratio = (distraction[i] / bucket).clamp(0.0, 1.0);
            let sustained = (runs[i] as f64 / 6.0).min(1.0);
            let focus_strength = focus_ratio * (0.35 + 0.65 * sustained);
            let distraction_strength = (distraction_ratio * 1.25).min(1.0);
            let switch_strength = (switches[i] / 2.0).min(1.0) * 0.22;
            (distraction_strength + switch_strength - focus_strength).clamp(-1.0, 1.0)
        })
        .collect();
    if raw.len() <= 2 {
        return raw;
    }
    (0..raw.len())
        .map(|i| {
            let v = raw[i];
            if v >= 0.0 {
                return v;
            }
            let lo = i.saturating_sub(1);
            let hi = (i + 1).min(raw.len() - 1);
            let neighbors: Vec<f64> = raw[lo..=hi].iter().copied().filter(|x| *x < 0.0).collect();
            if neighbors.len() <= 1 {
                return v;
            }
            let avg = neighbors.iter().sum::<f64>() / neighbors.len() as f64;
            (v * 0.55 + avg * 0.45).clamp(-1.0, 1.0)
        })
        .collect()
}

fn heat_values(facts: &[&Seg], vs: f64, count: usize) -> Vec<f64> {
    let mut focus = vec![0.0; count];
    let mut distraction = vec![0.0; count];
    let mut switches = vec![0.0; count];
    let ve = vs + count as f64 * HEAT_BUCKET;
    let sig = |s: &Seg| format!("{}|{}", s.cat.key, app_of(s.card).map(|a| a.0).unwrap_or_default());
    let mut prev: Option<(&Seg, f64)> = None;
    for &s in facts {
        let (start, end) = stats::offsets(s.start, s.end);
        if let Some((p, pend)) = prev {
            if start - pend <= 20.0 && sig(s) != sig(p) && start >= vs && start < ve {
                switches[((start - vs) / HEAT_BUCKET) as usize] += 1.0;
            }
        }
        if is_full_distraction(s) {
            add_interval(&mut distraction, start, end, vs, HEAT_BUCKET, 1.0);
        } else {
            add_interval(&mut focus, start, end, vs, HEAT_BUCKET, 1.0);
            for d in &s.card.distractions {
                if let Some((a, b)) = stats::distraction_span(s.card, d) {
                    let (a, b) = (a.max(s.start), b.min(s.end));
                    if b > a {
                        let (x, y) = stats::offsets(a, b);
                        add_interval(&mut distraction, x, y, vs, HEAT_BUCKET, 1.0);
                        add_interval(&mut focus, x, y, vs, HEAT_BUCKET, -1.0);
                    }
                }
            }
        }
        prev = Some((s, end));
    }
    heat_scores(&focus, &distraction, &switches, HEAT_BUCKET)
}

/// weeklyActivityWindow: earliest start and latest end ±30 minutes, snapped to 15 minutes, within
/// the logical day; 9 AM to 10 PM when the week is empty.
fn activity_window(positions: &[(f64, f64)]) -> (f64, f64) {
    let fallback = (5.0 * 60.0, 18.0 * 60.0);
    if positions.is_empty() {
        return fallback;
    }
    let earliest = positions.iter().map(|p| p.0).fold(f64::MAX, f64::min);
    let latest = positions.iter().map(|p| p.1).fold(f64::MIN, f64::max);
    let start = ((earliest - 30.0).max(0.0) / 15.0).floor() * 15.0;
    let end = ((latest + 30.0).min(stats::DAY_MINUTES) / 15.0).ceil() * 15.0;
    if end > start { (start, end) } else { fallback }
}

fn bucket_of(s: &Seg) -> (String, String, String) {
    if s.cat.key.contains("distraction") {
        ("distraction".into(), "Distraction".into(), DISTRACTION_COLOR.into())
    } else {
        (s.cat.key.clone(), s.cat.name.clone(), s.cat.color.clone())
    }
}

fn workflow_cell(facts: &[&Seg], ss: f64) -> Option<WeekCell> {
    let se = ss + WORKFLOW_SLOT;
    let mut buckets: Vec<((String, String, String), f64)> = Vec::new();
    for s in facts {
        let (a, b) = stats::offsets(s.start, s.end);
        let o = (b.min(se) - a.max(ss)).max(0.0);
        if o <= 0.0 {
            continue;
        }
        let k = bucket_of(s);
        match buckets.iter_mut().find(|(bk, _)| bk.0 == k.0) {
            Some((_, m)) => *m += o,
            None => buckets.push((k, o)),
        }
    }
    let total: f64 = buckets.iter().map(|b| b.1).sum();
    buckets.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.1.to_lowercase().cmp(&b.0.1.to_lowercase())));
    let ((key, name, color), _) = buckets.into_iter().next()?;
    Some(WeekCell { key, name, color, occupancy: (total / WORKFLOW_SLOT).min(1.0), secs: (total * 60.0).round() as i64 })
}

fn add_hours(row: &mut [i64], start: i64, end: i64) {
    let (so, eo) = stats::offsets(start, end);
    let mut h = (so / 60.0).floor() as usize;
    while h < row.len() && (h as f64) * 60.0 < eo {
        let (hs, he) = (h as f64 * 60.0, (h + 1) as f64 * 60.0);
        row[h] += ((eo.min(he) - so.max(hs)).max(0.0) * 60.0).round() as i64;
        h += 1;
    }
}

fn sort_totals(v: &mut [CategoryTotal]) {
    v.sort_by(|a, b| b.secs.cmp(&a.secs).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
}

fn add_total(v: &mut Vec<CategoryTotal>, t: CategoryTotal) {
    match v.iter_mut().find(|x| x.key == t.key) {
        Some(x) => x.secs += t.secs,
        None => v.push(t),
    }
}

/// App seconds per (category key, app key) over de-overlapped, non-idle time.
fn app_seconds(days: &[DayInput], cats: &[Category]) -> Vec<(stats::CatInfo, String, String, i64)> {
    let mut out: Vec<(stats::CatInfo, String, String, i64)> = Vec::new();
    for d in days {
        let segs = stats::segments(&d.cards, cats, d.start, d.end);
        for s in stats::remove_overlaps(&segs) {
            if s.cat.kind == Kind::Idle {
                continue;
            }
            let Some((key, name)) = app_of(s.card) else { continue };
            let secs = s.end - s.start;
            match out.iter_mut().find(|(c, k, _, _)| c.key == s.cat.key && *k == key) {
                Some(e) => e.3 += secs,
                None => out.push((s.cat.clone(), key, name, secs)),
            }
        }
    }
    out
}

pub fn compute_week(days: &[DayInput], previous: &[DayInput], cats: &[Category]) -> WeekStats {
    let mut summaries = Vec::new();
    let mut distribution: Vec<CategoryTotal> = Vec::new();
    let (mut captured, mut focus, mut distracted, mut idle) = (0, 0, 0, 0);
    let mut blocks: Vec<WeekBlock> = Vec::new();
    let mut day_segs: Vec<Vec<Seg>> = Vec::new();
    let mut hours: Vec<HourRow> = Vec::new();

    for d in days {
        let st = stats::compute_day(&d.day, &d.cards, cats, d.start, d.end);
        let segs = stats::segments(&d.cards, cats, d.start, d.end);
        captured += st.captured_secs;
        focus += st.focus_secs;
        distracted += st.distracted_secs;
        idle += st.idle_secs;
        for t in st.totals.iter().filter(|t| t.kind != Kind::Idle && t.kind != Kind::System) {
            add_total(&mut distribution, t.clone());
        }
        blocks.extend(st.focus_blocks.iter().map(|b| WeekBlock { day: d.day.clone(), start_ts: b.start_ts, end_ts: b.end_ts, secs: b.secs }));

        // Context shifts and distraction counts (WeeklyDashboardBuilder.buildContextCharts).
        let mut shifts = 0;
        let mut count = 0;
        let mut prev: Option<&str> = None;
        for s in segs.iter().filter(|s| s.cat.kind != Kind::Idle) {
            if prev.is_some_and(|p| p != s.cat.key) {
                shifts += 1;
            }
            prev = Some(s.cat.key.as_str());
            if !s.card.distractions.is_empty() {
                count += s.card.distractions.iter().filter(|x| crate::timeutil::parse_clock(&x.start_time).is_some()).count() as u32;
            } else if is_full_distraction(s) {
                count += 1;
            }
        }

        // Distracted time per clock hour: Distraction cards plus distractions noted in other cards.
        let mut spans: Vec<(i64, i64)> = Vec::new();
        for s in &segs {
            if s.cat.kind == Kind::Distraction {
                spans.push((s.start, s.end));
            } else if s.cat.kind == Kind::Focus {
                for x in &s.card.distractions {
                    if let Some((a, b)) = stats::distraction_span(s.card, x) {
                        let (a, b) = (a.max(s.start), b.min(s.end));
                        if b > a {
                            spans.push((a, b));
                        }
                    }
                }
            }
        }
        let mut row = vec![0i64; 24];
        for (a, b) in stats::merge_intervals(spans, 0) {
            add_hours(&mut row, a, b);
        }
        hours.push(HourRow { day: d.day.clone(), label: weekday_label(&d.day, false), secs: row });

        summaries.push(WeekDaySummary {
            day: d.day.clone(),
            label: weekday_label(&d.day, false),
            captured_secs: st.captured_secs,
            focus_secs: st.focus_secs,
            distracted_secs: st.distracted_secs,
            idle_secs: st.idle_secs,
            totals: st.totals,
            context_shifts: shifts,
            distraction_count: count,
            longest_focus: st.longest_focus,
        });
        day_segs.push(segs);
    }

    // Distribution: top four plus Other.
    sort_totals(&mut distribution);
    if distribution.len() > 5 {
        let other: i64 = distribution[4..].iter().map(|t| t.secs).sum();
        distribution.truncate(4);
        distribution.push(CategoryTotal { key: "other".into(), name: "Other".into(), color: OTHER_COLOR.into(), kind: Kind::Focus, secs: other });
    }

    // Workflow grid and heat map share the activity window of all non-idle cards.
    let facts: Vec<Vec<&Seg>> = day_segs.iter().map(|v| v.iter().filter(|s| s.cat.kind != Kind::Idle).collect()).collect();
    let positions: Vec<(f64, f64)> = facts.iter().flatten().map(|s| stats::offsets(s.start, s.end)).collect();
    let (ws, we) = activity_window(&positions);
    let slots = (((we - ws) / WORKFLOW_SLOT) as usize).max(1);
    let mut wf_totals: Vec<CategoryTotal> = Vec::new();
    let rows = days
        .iter()
        .zip(&facts)
        .map(|(d, f)| {
            for s in f {
                let (key, name, color) = bucket_of(s);
                add_total(&mut wf_totals, CategoryTotal { key, name, color, kind: s.cat.kind, secs: s.end - s.start });
            }
            WeekGridRow {
                day: d.day.clone(),
                label: weekday_label(&d.day, false),
                cells: (0..slots).map(|i| workflow_cell(f, ws + i as f64 * WORKFLOW_SLOT)).collect(),
            }
        })
        .collect();
    sort_totals(&mut wf_totals);
    wf_totals.truncate(7);
    let buckets = (((we - ws) / HEAT_BUCKET).ceil() as usize).max(1);
    let heat_rows = days
        .iter()
        .zip(&facts)
        .map(|(d, f)| HeatRow { day: d.day.clone(), label: weekday_label(&d.day, false), values: heat_values(f, ws, buckets) })
        .collect();

    // "{day} had the most interruptions, ..." (first busiest day wins ties, as in Swift's max(by:)).
    let busiest = summaries.iter().fold(None::<&WeekDaySummary>, |best, s| match best {
        Some(b) if b.context_shifts + b.distraction_count >= s.context_shifts + s.distraction_count => Some(b),
        _ => Some(s),
    });
    let context_insight = match busiest {
        Some(b) if b.context_shifts + b.distraction_count > 0 => format!(
            "{} had the most interruptions, with {} context shifts and {} distractions.",
            weekday_label(&b.day, true),
            b.context_shifts,
            b.distraction_count
        ),
        _ => "No context shift or distraction pattern was detected in this week.".to_string(),
    };

    blocks.sort_by(|a, b| b.secs.cmp(&a.secs).then(a.start_ts.cmp(&b.start_ts)));
    blocks.truncate(5);

    // Apps per category with the week before, and overall top apps.
    let current = app_seconds(days, cats);
    let before = app_seconds(previous, cats);
    let mut by_cat: Vec<AppCategory> = Vec::new();
    for (cat, key, name, secs) in &current {
        let prev = before.iter().filter(|(c, k, _, _)| c.key == cat.key && k == key).map(|e| e.3).sum();
        let app = AppUsage { name: name.clone(), secs: *secs, previous_secs: prev };
        match by_cat.iter_mut().find(|c| c.key == cat.key) {
            Some(c) => {
                c.secs += secs;
                c.apps.push(app);
            }
            None => by_cat.push(AppCategory { key: cat.key.clone(), name: cat.name.clone(), color: cat.color.clone(), secs: *secs, apps: vec![app] }),
        }
    }
    for c in by_cat.iter_mut() {
        c.apps.sort_by(|a, b| b.secs.cmp(&a.secs).then(a.name.cmp(&b.name)));
        c.apps.truncate(8);
    }
    by_cat.sort_by(|a, b| b.secs.cmp(&a.secs).then(a.name.cmp(&b.name)));
    by_cat.truncate(5);
    let mut top: Vec<(String, AppUsage)> = Vec::new();
    for (_, key, name, secs) in &current {
        match top.iter_mut().find(|(k, _)| k == key) {
            Some((_, a)) => a.secs += secs,
            None => {
                let prev = before.iter().filter(|(_, k, _, _)| k == key).map(|e| e.3).sum();
                top.push((key.clone(), AppUsage { name: name.clone(), secs: *secs, previous_secs: prev }));
            }
        }
    }
    let mut top_apps: Vec<AppUsage> = top.into_iter().map(|(_, a)| a).collect();
    top_apps.sort_by(|a, b| b.secs.cmp(&a.secs).then(a.name.cmp(&b.name)));
    top_apps.truncate(10);

    WeekStats {
        week_start: days.first().map(|d| d.day.clone()).unwrap_or_default(),
        week_end: days.last().map(|d| d.day.clone()).unwrap_or_default(),
        days: summaries,
        distribution,
        captured_secs: captured,
        focus_secs: focus,
        distracted_secs: distracted,
        idle_secs: idle,
        workflow: WeekGrid { start_minute: ws, end_minute: ws + slots as f64 * WORKFLOW_SLOT, slot_minutes: WORKFLOW_SLOT, rows, totals: wf_totals },
        heatmap: Heatmap { start_minute: ws, end_minute: ws + buckets as f64 * HEAT_BUCKET, bucket_minutes: HEAT_BUCKET, rows: heat_rows },
        distraction_hours: hours,
        context_insight,
        focus_blocks: blocks,
        apps_by_category: by_cat,
        top_apps,
        programs: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::insights::stats::tests::{at, card, distraction};
    use crate::timeutil::logical_day_bounds;
    use crate::types::{default_categories, AppSites};

    fn input(day: &str, cards: Vec<StoredCard>) -> DayInput {
        let (start, end) = logical_day_bounds(day).unwrap();
        DayInput { day: day.into(), start, end, cards }
    }

    fn with_app(mut c: StoredCard, app: &str) -> StoredCard {
        c.app_sites = Some(AppSites { primary: Some(app.into()), secondary: None });
        c
    }

    fn week(cards_by_day: Vec<Vec<StoredCard>>) -> Vec<DayInput> {
        let days = week_days("2026-10-14").unwrap();
        days.iter().zip(cards_by_day.into_iter().chain(std::iter::repeat_with(Vec::new))).map(|(d, c)| input(d, c)).collect()
    }

    #[test]
    fn week_runs_monday_to_sunday() {
        let d = week_days("2026-10-14").unwrap();
        assert_eq!(d.first().unwrap(), "2026-10-12");
        assert_eq!(d.last().unwrap(), "2026-10-18");
        assert_eq!(week_days("2026-10-18").unwrap()[0], "2026-10-12");
        assert_eq!(week_days("2026-10-19").unwrap()[0], "2026-10-19");
    }

    #[test]
    fn aggregates_days_and_distribution() {
        let mon = (2026, 10, 12);
        let tue = (2026, 10, 13);
        let mut w = card(1, at(mon, 9, 0), at(mon, 11, 0), "Work", "Spec");
        w.distractions = vec![distraction(at(mon, 9, 30), at(mon, 9, 40), "X")];
        let days = week(vec![
            vec![w, card(2, at(mon, 11, 0), at(mon, 11, 30), "Distraction", "YouTube"), card(3, at(mon, 12, 0), at(mon, 13, 0), "Idle", "Idle")],
            vec![card(4, at(tue, 14, 0), at(tue, 15, 0), "Personal", "Bills"), card(5, at(tue, 15, 0), at(tue, 15, 30), "Work", "Mail")],
        ]);
        let st = compute_week(&days, &[], &default_categories());
        assert_eq!((st.week_start.as_str(), st.week_end.as_str()), ("2026-10-12", "2026-10-18"));
        assert_eq!(st.days.len(), 7);
        let dist: Vec<(&str, i64)> = st.distribution.iter().map(|t| (t.name.as_str(), t.secs / 60)).collect();
        assert_eq!(dist, vec![("Work", 150), ("Personal", 60), ("Distraction", 30)]);
        assert_eq!(st.idle_secs / 60, 60);
        assert_eq!(st.distracted_secs / 60, 40);
        assert_eq!(st.focus_secs / 60, 150 + 60 - 10);
        assert_eq!(st.days[0].context_shifts, 1, "Work → Distraction (Idle is left out)");
        assert_eq!(st.days[0].distraction_count, 2, "one noted distraction and one Distraction card");
        assert_eq!(st.days[1].context_shifts, 1);
        assert_eq!(st.context_insight, "Monday had the most interruptions, with 1 context shifts and 2 distractions.");
        assert_eq!(st.focus_blocks[0].secs, 120 * 60);
        assert_eq!(st.focus_blocks[0].day, "2026-10-12");
        // Distraction hours: 9:30-9:40 lands in the 9 AM bucket (index 5), 11:00-11:30 in index 7
        assert_eq!(st.distraction_hours[0].secs[5], 600);
        assert_eq!(st.distraction_hours[0].secs[7], 1800);
    }

    #[test]
    fn distribution_folds_into_other() {
        let mon = (2026, 10, 12);
        let cats: Vec<Category> = ["A", "B", "C", "D", "E", "F"]
            .iter()
            .map(|n| Category { name: n.to_string(), color: "#000000".into(), description: String::new(), is_idle: false })
            .collect();
        let cards = (0..6).map(|i| card(i, at(mon, 8 + i as u32, 0), at(mon, 8 + i as u32, 10 + i as u32 * 5), ["A", "B", "C", "D", "E", "F"][i as usize], "x")).collect();
        let st = compute_week(&week(vec![cards]), &[], &cats);
        let names: Vec<&str> = st.distribution.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, vec!["F", "E", "D", "C", "Other"]);
        assert_eq!(st.distribution[4].secs, (10 + 15) * 60);
    }

    #[test]
    fn workflow_grid_uses_dominant_bucket() {
        let mon = (2026, 10, 12);
        let days = week(vec![vec![
            card(1, at(mon, 9, 0), at(mon, 9, 10), "Work", "a"),
            card(2, at(mon, 9, 10), at(mon, 9, 15), "Distraction", "b"),
        ]]);
        let st = compute_week(&days, &[], &default_categories());
        let g = &st.workflow;
        // 9:00 - 30 min = 8:30 → 270 minutes after 4 AM; 9:15 + 30 = 9:45 → 345
        assert_eq!((g.start_minute, g.end_minute), (270.0, 345.0));
        assert_eq!(g.rows[0].cells.len(), 5);
        let c = g.rows[0].cells[2].as_ref().unwrap();
        assert_eq!((c.name.as_str(), c.occupancy, c.secs), ("Work", 1.0, 900));
        assert!(g.rows[1].cells.iter().all(|c| c.is_none()));
        assert_eq!(st.heatmap.rows[0].values.len(), 15);
    }

    #[test]
    fn heat_scores_follow_dayflow() {
        // A full focus bucket on its own: run of 1 → 0.35 + 0.65/6; no smoothing with < 3 values.
        let s = heat_scores(&[5.0], &[0.0], &[0.0], 5.0);
        assert!((s[0] + 0.35 + 0.65 / 6.0).abs() < 1e-9, "{s:?}");
        // A full distraction bucket: min(1, 1.25) = 1.
        assert_eq!(heat_scores(&[0.0], &[5.0], &[0.0], 5.0), vec![1.0]);
        // Six clean focus buckets in a row reach full strength, then smoothing keeps -1.
        let s = heat_scores(&[5.0; 6], &[0.0; 6], &[0.0; 6], 5.0);
        assert!(s.iter().all(|v| (*v + 1.0).abs() < 1e-9), "{s:?}");
        // A switch adds 0.11 per switch (0.22 at two).
        let s = heat_scores(&[0.0, 0.0, 0.0], &[0.0; 3], &[2.0, 0.0, 0.0], 5.0);
        assert!((s[0] - 0.22).abs() < 1e-9);
    }

    #[test]
    fn apps_with_previous_week() {
        let mon = (2026, 10, 12);
        let prev_mon = (2026, 10, 5);
        let days = week(vec![vec![
            with_app(card(1, at(mon, 9, 0), at(mon, 10, 0), "Work", "PR"), "github.com"),
            with_app(card(2, at(mon, 10, 0), at(mon, 10, 30), "Work", "Docs"), "docs.rs"),
            with_app(card(3, at(mon, 11, 0), at(mon, 11, 20), "Distraction", "Videos"), "https://www.youtube.com/watch"),
            card(4, at(mon, 12, 0), at(mon, 13, 0), "Work", "No app"),
        ]]);
        let prev_days = week_days("2026-10-05").unwrap();
        let previous: Vec<DayInput> = prev_days
            .iter()
            .enumerate()
            .map(|(i, d)| input(d, if i == 0 { vec![with_app(card(9, at(prev_mon, 9, 0), at(prev_mon, 9, 45), "Work", "PR"), "GitHub")] } else { vec![] }))
            .collect();
        let st = compute_week(&days, &previous, &default_categories());
        let top: Vec<(&str, i64, i64)> = st.top_apps.iter().map(|a| (a.name.as_str(), a.secs / 60, a.previous_secs / 60)).collect();
        assert_eq!(top, vec![("GitHub", 60, 45), ("Docs", 30, 0), ("YouTube", 20, 0)]);
        assert_eq!(st.apps_by_category[0].name, "Work");
        assert_eq!(st.apps_by_category[0].apps.len(), 2);
        assert_eq!(st.apps_by_category[1].apps[0].name, "YouTube");
    }

    #[test]
    fn app_names() {
        assert_eq!(pretty_app_name("github.com/foo"), "GitHub");
        assert_eq!(pretty_app_name("docs.rs"), "Docs");
        assert_eq!(pretty_app_name("Visual Studio Code"), "VS Code");
        assert_eq!(pretty_app_name("obsidian"), "Obsidian");
        assert_eq!(pretty_app_name("  "), "Other");
    }

    #[test]
    fn empty_week() {
        let st = compute_week(&week(vec![]), &[], &default_categories());
        assert_eq!(st.context_insight, "No context shift or distraction pattern was detected in this week.");
        assert_eq!((st.workflow.start_minute, st.workflow.end_minute), (300.0, 1080.0));
        assert!(st.distribution.is_empty() && st.top_apps.is_empty() && st.focus_blocks.is_empty());
    }
}
