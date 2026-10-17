//! Day statistics without a model call: time per category, the activity grid, focus blocks and
//! distractions. Ported from Dayflow (MIT, (c) 2025 Jerry Liu):
//! Views/UI/DailyWorkflowComputation.swift (grid, stat chips, distraction markers) and
//! Views/Components/DaySummaryStats.swift (overlap removal, focus blocks, captured and distracted
//! time). What a category counts as (focus, distraction or neither) is its "Counts as" choice in
//! Settings > Categories (`Category::counts`); without a choice it follows Dayflow's default day
//! plan (Models/DayGoalPlan.swift): every category except Distraction, Idle and System is focus.
//!
//! Percentages of the day use active time (recorded minus idle) as the denominator everywhere.
//!
//! Positions on the grid are wall-clock minutes since 4 AM, so hour labels match the clock;
//! durations come from the stored Unix timestamps.

use crate::db::StoredCard;
use crate::timeutil;
use crate::types::{Category, CountsAs, Distraction};
use chrono::Timelike;
use serde::Serialize;

pub const SLOT_MINUTES: f64 = 15.0;
pub const DAY_MINUTES: f64 = 1440.0;
const FOCUS_GAP_SECS: i64 = 5 * 60;
const MARKER_MERGE_MINUTES: f64 = 2.0;
/// Dayflow's empty-day window, 9 AM to 9 PM (as minutes since 4 AM).
const PLACEHOLDER_WINDOW: (f64, f64) = (5.0 * 60.0, 17.0 * 60.0);
const MIN_WINDOW_MINUTES: f64 = 12.0 * 60.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Focus,
    Distraction,
    /// A category set to count as neither focus nor distraction (still active time).
    Neither,
    Idle,
    System,
}

impl From<CountsAs> for Kind {
    fn from(c: CountsAs) -> Kind {
        match c {
            CountsAs::Focus => Kind::Focus,
            CountsAs::Distraction => Kind::Distraction,
            CountsAs::Neither => Kind::Neither,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatInfo {
    pub key: String,
    pub name: String,
    pub color: String,
    pub kind: Kind,
    #[serde(skip)]
    pub order: usize,
}

/// What a card's category name counts as under the settings' categories: the category's choice,
/// or for a name that is not in the settings the rule by name. Failed cards ("System") and the
/// idle category count as neither. Shared by the views' numbers, Goals and daytrace-mcp.
pub fn counts_as(name: &str, cats: &[Category]) -> CountsAs {
    let key = name.trim().to_lowercase();
    if key == "system" {
        return CountsAs::Neither;
    }
    match cats.iter().find(|c| c.name.trim().to_lowercase() == key) {
        Some(c) => c.counts(),
        None => CountsAs::derived(&key, key == "idle"),
    }
}

/// Minutes of the categories that count as focus, from per-category totals (daytrace-mcp's
/// get_weekly and the CLI's `weekly`).
pub fn focus_minutes(totals: &[(String, i64)], cats: &[Category]) -> i64 {
    totals.iter().filter(|(name, _)| counts_as(name, cats) == CountsAs::Focus).map(|(_, m)| m).sum()
}

/// Dayflow's fallback palette for categories that are not in the settings (djb2 hash).
pub fn fallback_color(key: &str) -> String {
    const PALETTE: [&str; 4] = ["#B984FF", "#6AADFF", "#FF5950", "#A0AEC0"];
    let mut h: i64 = 5381;
    for b in key.bytes() {
        h = (h << 5).wrapping_add(h).wrapping_add(b as i64);
    }
    PALETTE[(h.unsigned_abs() % PALETTE.len() as u64) as usize].to_string()
}

/// Resolves a card's category against the settings.
pub fn cat_info(raw: &str, cats: &[Category]) -> CatInfo {
    let trimmed = raw.trim();
    let name = if trimmed.is_empty() { "Uncategorized" } else { trimmed };
    let key = name.to_lowercase();
    if key == "system" {
        return CatInfo { key, name: name.into(), color: "#B4443A".into(), kind: Kind::System, order: usize::MAX };
    }
    if let Some((i, c)) = cats.iter().enumerate().find(|(_, c)| c.name.trim().to_lowercase() == key) {
        let kind = if c.is_idle { Kind::Idle } else { c.counts().into() };
        return CatInfo { key, name: c.name.trim().to_string(), color: c.color.clone(), kind, order: i };
    }
    let kind = if key == "idle" { Kind::Idle } else { CountsAs::derived(&key, false).into() };
    CatInfo { color: fallback_color(&key), key, name: name.into(), kind, order: usize::MAX }
}

/// Minutes since 4 AM on the wall clock (0 ≤ m < 1440).
pub fn wall_offset(ts: i64) -> f64 {
    let t = timeutil::local(ts);
    let m = (t.hour() * 60 + t.minute()) as f64 + t.second() as f64 / 60.0;
    let off = m - (timeutil::DAY_START_HOUR * 60) as f64;
    if off < 0.0 { off + DAY_MINUTES } else { off }
}

/// Wall-clock position of [start, end): start offset plus the real duration.
pub fn offsets(start: i64, end: i64) -> (f64, f64) {
    let s = wall_offset(start);
    (s, s + (end - start) as f64 / 60.0)
}

/// A card clipped to a day.
#[derive(Debug, Clone)]
pub struct Seg<'a> {
    pub card: &'a StoredCard,
    pub start: i64,
    pub end: i64,
    pub cat: CatInfo,
}

/// Non-System cards clipped to [day_start, day_end), sorted by start.
pub fn segments<'a>(cards: &'a [StoredCard], cats: &[Category], day_start: i64, day_end: i64) -> Vec<Seg<'a>> {
    let mut out: Vec<Seg> = cards
        .iter()
        .filter_map(|c| {
            let cat = cat_info(&c.category, cats);
            if cat.kind == Kind::System {
                return None;
            }
            let (s, e) = (c.start_ts.max(day_start), c.end_ts.min(day_end));
            (e > s).then_some(Seg { card: c, start: s, end: e, cat })
        })
        .collect();
    out.sort_by(|a, b| a.start.cmp(&b.start).then(a.end.cmp(&b.end)));
    out
}

/// DaySummaryStats.removeOverlaps: later cards are trimmed so each second counts once. Equal
/// starts keep the longer card.
pub fn remove_overlaps<'a>(segs: &[Seg<'a>]) -> Vec<Seg<'a>> {
    let mut sorted = segs.to_vec();
    sorted.sort_by(|a, b| a.start.cmp(&b.start).then(b.end.cmp(&a.end)).then(a.card.id.cmp(&b.card.id)));
    let mut out = Vec::with_capacity(sorted.len());
    let mut covered = i64::MIN;
    for s in sorted {
        let start = s.start.max(covered);
        if s.end > start {
            covered = covered.max(s.end);
            out.push(Seg { start, ..s });
        }
    }
    out
}

/// Merge overlapping intervals and those closer than `gap` seconds.
pub fn merge_intervals(mut v: Vec<(i64, i64)>, gap: i64) -> Vec<(i64, i64)> {
    v.sort();
    let mut out: Vec<(i64, i64)> = Vec::new();
    for (s, e) in v {
        match out.last_mut() {
            Some(last) if s - last.1 < gap.max(0) || s <= last.1 => last.1 = last.1.max(e),
            _ => out.push((s, e)),
        }
    }
    out
}

/// Unix range of a distraction noted inside a card. Dayflow's normalizedMiniDistractionRange:
/// times outside the card collapse to one minute at the nearest valid point.
pub fn distraction_span(card: &StoredCard, d: &Distraction) -> Option<(i64, i64)> {
    if card.end_ts <= card.start_ts {
        return None;
    }
    let anchor = (card.start_ts + card.end_ts) / 2;
    let (s, e) = timeutil::resolve_card_range(&d.start_time, &d.end_time, anchor)?;
    if e > s && s >= card.start_ts && e <= card.end_ts {
        return Some((s, e));
    }
    let latest_start = card.start_ts.max(card.end_ts - 60);
    let cs = s.max(card.start_ts).min(latest_start);
    let ce = card.end_ts.min(cs + 60);
    (ce > cs).then_some((cs, ce))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CategoryTotal {
    pub key: String,
    pub name: String,
    pub color: String,
    pub kind: Kind,
    pub secs: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridCell {
    pub occupancy: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridRow {
    pub key: String,
    pub name: String,
    pub color: String,
    pub cells: Vec<GridCell>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Marker {
    pub start_minute: f64,
    pub end_minute: f64,
    pub title: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Grid {
    /// Minutes since 4 AM.
    pub start_minute: f64,
    pub end_minute: f64,
    pub slot_minutes: f64,
    pub rows: Vec<GridRow>,
    /// Index into `rows` of the category that fills most of each slot.
    pub dominant: Vec<Option<usize>>,
    pub markers: Vec<Marker>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DistractionItem {
    pub start_ts: i64,
    pub end_ts: i64,
    pub secs: i64,
    pub title: String,
    pub summary: String,
    /// Title of the card the distraction was noted in; None for a Distraction-category card.
    pub within_card: Option<String>,
    pub card_id: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub start_ts: i64,
    pub end_ts: i64,
    pub secs: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NamedSecs {
    pub name: String,
    pub secs: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DayStats {
    pub day: String,
    pub day_start_ts: i64,
    pub day_end_ts: i64,
    pub card_count: usize,
    /// All non-System time, overlaps counted once (Dayflow's "Total time captured"; "Recorded").
    pub captured_secs: i64,
    /// Recorded minus idle: the denominator of every percentage of the day.
    pub active_secs: i64,
    pub focus_secs: i64,
    pub distracted_secs: i64,
    pub idle_secs: i64,
    /// Gaps between cards (Dayflow's "Transitioning time").
    pub transition_secs: i64,
    pub context_switches: u32,
    /// Cards with distractions noted inside them (Dayflow's "Interrupted").
    pub interruptions: u32,
    /// Distracted time as a share of active time.
    pub distracted_ratio: f64,
    pub totals: Vec<CategoryTotal>,
    pub grid: Grid,
    pub distractions: Vec<DistractionItem>,
    /// Focus time merged across gaps under 5 minutes, in time order.
    pub focus_blocks: Vec<Block>,
    pub longest_focus: Option<Block>,
    /// Foreground programs from screenshot metadata; filled by the command.
    pub programs: Vec<NamedSecs>,
}

/// Category totals with overlaps counted once, sorted like DaySummaryStats.computeCategoryDurations
/// (whole minutes descending, then settings order, then name).
pub fn category_totals(normalized: &[Seg]) -> Vec<CategoryTotal> {
    let mut totals: Vec<(CatInfo, i64)> = Vec::new();
    for s in normalized {
        match totals.iter_mut().find(|(c, _)| c.key == s.cat.key) {
            Some((_, secs)) => *secs += s.end - s.start,
            None => totals.push((s.cat.clone(), s.end - s.start)),
        }
    }
    totals.sort_by(|(a, x), (b, y)| {
        (y / 60).cmp(&(x / 60)).then(a.order.cmp(&b.order)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    totals
        .into_iter()
        .filter(|(_, s)| *s > 0)
        .map(|(c, secs)| CategoryTotal { key: c.key, name: c.name, color: c.color, kind: c.kind, secs })
        .collect()
}

/// Focus blocks: focus-category time merged across gaps shorter than 5 minutes.
pub fn focus_blocks(normalized: &[Seg]) -> Vec<Block> {
    let spans: Vec<(i64, i64)> = normalized.iter().filter(|s| s.cat.kind == Kind::Focus).map(|s| (s.start, s.end)).collect();
    let mut merged: Vec<(i64, i64)> = Vec::new();
    let mut sorted = spans;
    sorted.sort();
    for (s, e) in sorted {
        if let Some(last) = merged.last_mut() {
            if s - last.1 < FOCUS_GAP_SECS {
                last.1 = last.1.max(e);
                continue;
            }
        }
        merged.push((s, e));
    }
    merged.into_iter().map(|(s, e)| Block { start_ts: s, end_ts: e, secs: e - s }).collect()
}

pub fn longest(blocks: &[Block]) -> Option<Block> {
    blocks.iter().fold(None::<&Block>, |best, b| match best {
        Some(x) if x.secs >= b.secs => Some(x),
        _ => Some(b),
    }).cloned()
}

/// Distractions of the day: Distraction-category cards and distractions noted inside other cards.
pub fn distractions(segs: &[Seg]) -> Vec<DistractionItem> {
    let mut out = Vec::new();
    for s in segs {
        if s.cat.kind == Kind::Distraction {
            out.push(DistractionItem {
                start_ts: s.start,
                end_ts: s.end,
                secs: s.end - s.start,
                title: s.card.title.trim().to_string(),
                summary: s.card.summary.trim().to_string(),
                within_card: None,
                card_id: s.card.id,
            });
            continue;
        }
        for d in &s.card.distractions {
            let Some((ds, de)) = distraction_span(s.card, d) else { continue };
            let (ds, de) = (ds.max(s.start), de.min(s.end));
            if de <= ds {
                continue;
            }
            out.push(DistractionItem {
                start_ts: ds,
                end_ts: de,
                secs: de - ds,
                title: d.title.trim().to_string(),
                summary: d.summary.trim().to_string(),
                within_card: Some(s.card.title.trim().to_string()),
                card_id: s.card.id,
            });
        }
    }
    out.sort_by_key(|d| (d.start_ts, d.end_ts));
    out.dedup_by(|a, b| a.start_ts == b.start_ts && a.end_ts == b.end_ts && a.title == b.title);
    out
}

/// Seconds of distractions noted inside each focus segment (merged per segment).
fn embedded_secs(s: &Seg) -> i64 {
    let spans: Vec<(i64, i64)> = s
        .card
        .distractions
        .iter()
        .filter_map(|d| distraction_span(s.card, d))
        .map(|(a, b)| (a.max(s.start), b.min(s.end)))
        .filter(|(a, b)| b > a)
        .collect();
    merge_intervals(spans, 0).iter().map(|(a, b)| b - a).sum()
}

/// The Dayflow daily grid: one row per category, 15-minute slots, each cell the share of the slot
/// the category fills plus the card that fills most of it.
pub fn grid(segs: &[Seg], cats: &[Category]) -> Grid {
    let pos: Vec<(f64, f64)> = segs.iter().map(|s| offsets(s.start, s.end)).collect();
    let (start, end) = if pos.is_empty() {
        PLACEHOLDER_WINDOW
    } else {
        let first = pos.iter().map(|p| p.0).fold(f64::MAX, f64::min);
        let last = pos.iter().map(|p| p.1).fold(f64::MIN, f64::max);
        let start = (first / 60.0).floor() * 60.0;
        let data_end = (last / 60.0).ceil() * 60.0;
        let end = (start + MIN_WINDOW_MINUTES).max(data_end).min(DAY_MINUTES.max(data_end));
        (start, end)
    };
    let slots = (((end - start) / SLOT_MINUTES).ceil() as usize).max(1);

    // Rows: every configured category in settings order, then unknown categories in use.
    let mut keys: Vec<CatInfo> = Vec::new();
    for c in cats {
        let info = cat_info(&c.name, cats);
        if info.kind != Kind::System && !keys.iter().any(|k| k.key == info.key) {
            keys.push(info);
        }
    }
    let mut unknown: Vec<CatInfo> = segs.iter().map(|s| s.cat.clone()).filter(|c| !keys.iter().any(|k| k.key == c.key)).collect();
    unknown.sort_by(|a, b| a.key.cmp(&b.key));
    unknown.dedup_by(|a, b| a.key == b.key);
    keys.extend(unknown);

    let mut slot_fill = vec![vec![0.0f64; slots]; keys.len()];
    let rows: Vec<GridRow> = keys
        .iter()
        .enumerate()
        .map(|(ri, k)| {
            let cells = (0..slots)
                .map(|i| {
                    let ss = start + i as f64 * SLOT_MINUTES;
                    let se = (ss + SLOT_MINUTES).min(end);
                    let len = (se - ss).max(1.0);
                    let mut total = 0.0;
                    let mut best: Option<(f64, &StoredCard)> = None;
                    for (s, p) in segs.iter().zip(&pos) {
                        if s.cat.key != k.key {
                            continue;
                        }
                        let o = (p.1.min(se) - p.0.max(ss)).max(0.0);
                        total += o;
                        if o > 0.0 && best.map(|(b, _)| o > b).unwrap_or(true) {
                            best = Some((o, s.card));
                        }
                    }
                    slot_fill[ri][i] = total;
                    GridCell {
                        occupancy: (total / len).min(1.0),
                        title: best.map(|(_, c)| c.title.clone()),
                        card_id: best.map(|(_, c)| c.id),
                    }
                })
                .collect();
            GridRow { key: k.key.clone(), name: k.name.clone(), color: k.color.clone(), cells }
        })
        .collect();
    let dominant = (0..slots)
        .map(|i| {
            let mut best: Option<(usize, f64)> = None;
            for (ri, row) in slot_fill.iter().enumerate() {
                if row[i] > 0.0 && best.map(|(_, b)| row[i] > b).unwrap_or(true) {
                    best = Some((ri, row[i]));
                }
            }
            best.map(|(ri, _)| ri)
        })
        .collect();

    // Distraction markers: Distraction-category cards plus noted distractions, merged when they
    // overlap or are within 2 minutes, titles joined.
    let mut marks: Vec<Marker> = Vec::new();
    for s in segs {
        if s.cat.kind == Kind::Distraction {
            let (a, b) = offsets(s.start, s.end);
            marks.push(Marker { start_minute: a, end_minute: b, title: s.card.title.clone() });
        }
        for d in &s.card.distractions {
            if let Some((a, b)) = distraction_span(s.card, d) {
                let (a, b) = (a.max(s.start), b.min(s.end));
                if b > a {
                    let (x, y) = offsets(a, b);
                    marks.push(Marker { start_minute: x, end_minute: y, title: d.title.clone() });
                }
            }
        }
    }
    marks.retain(|m| m.end_minute > start && m.start_minute < end);
    for m in marks.iter_mut() {
        m.start_minute = m.start_minute.max(start);
        m.end_minute = m.end_minute.min(end);
    }
    marks.sort_by(|a, b| a.start_minute.total_cmp(&b.start_minute));
    let mut markers: Vec<Marker> = Vec::new();
    let mut titles: Vec<String> = Vec::new();
    for m in marks {
        match markers.last_mut() {
            Some(cur) if m.start_minute <= cur.end_minute + MARKER_MERGE_MINUTES => {
                cur.end_minute = cur.end_minute.max(m.end_minute);
                if !titles.contains(&m.title) {
                    titles.push(m.title.clone());
                    cur.title = titles.join(", ");
                }
            }
            _ => {
                titles = vec![m.title.clone()];
                markers.push(m);
            }
        }
    }

    Grid { start_minute: start, end_minute: end, slot_minutes: SLOT_MINUTES, rows, dominant, markers }
}

/// Focus and distraction of de-overlapped segments: time in focus categories (minus the
/// distractions noted inside those cards), and time in distraction categories plus the noted
/// distractions. Daily, Weekly, Goals and Your day so far all use these numbers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FocusSplit {
    pub captured: i64,
    pub focus: i64,
    pub distracted: i64,
    pub idle: i64,
    /// Focus seconds per focus category (key, name, color, secs), in segment order.
    pub focus_by_category: Vec<(String, String, String, i64)>,
}

impl FocusSplit {
    /// Recorded minus idle.
    pub fn active(&self) -> i64 {
        (self.captured - self.idle).max(0)
    }
}

pub fn focus_split(normalized: &[Seg]) -> FocusSplit {
    let mut f = FocusSplit::default();
    for s in normalized {
        let d = s.end - s.start;
        f.captured += d;
        match s.cat.kind {
            Kind::Focus => {
                let inside = embedded_secs(s).min(d);
                f.focus += d - inside;
                f.distracted += inside;
                match f.focus_by_category.iter_mut().find(|c| c.0 == s.cat.key) {
                    Some(c) => c.3 += d - inside,
                    None => f.focus_by_category.push((s.cat.key.clone(), s.cat.name.clone(), s.cat.color.clone(), d - inside)),
                }
            }
            Kind::Distraction => f.distracted += d,
            // Noted distractions inside a card that counts as neither are distracted time too.
            Kind::Neither => f.distracted += embedded_secs(s).min(d),
            Kind::Idle => f.idle += d,
            Kind::System => {}
        }
    }
    f
}

/// Everything the Daily view shows for one logical day, without any model call.
pub fn compute_day(day: &str, cards: &[StoredCard], cats: &[Category], day_start: i64, day_end: i64) -> DayStats {
    let segs = segments(cards, cats, day_start, day_end);
    let normalized = remove_overlaps(&segs);
    let split = focus_split(&normalized);
    let (captured, focus, distracted, idle, active) = (split.captured, split.focus, split.distracted, split.idle, split.active());

    // Chips from DailyWorkflowComputation, over the raw (not de-overlapped) segments.
    let mut switches = 0;
    let mut interruptions = 0;
    let mut transition = 0;
    let mut prev_key: Option<&str> = None;
    let mut prev_end: Option<i64> = None;
    for s in &segs {
        if !s.card.distractions.is_empty() {
            interruptions += 1;
        }
        if let Some(k) = prev_key {
            if k != s.cat.key {
                switches += 1;
            }
        }
        prev_key = Some(s.cat.key.as_str());
        prev_end = Some(match prev_end {
            Some(pe) => {
                if s.start > pe {
                    transition += s.start - pe;
                }
                pe.max(s.end)
            }
            None => s.end,
        });
    }

    let blocks = focus_blocks(&normalized);
    DayStats {
        day: day.to_string(),
        day_start_ts: day_start,
        day_end_ts: day_end,
        card_count: segs.len(),
        captured_secs: captured,
        active_secs: active,
        focus_secs: focus,
        distracted_secs: distracted,
        idle_secs: idle,
        transition_secs: transition,
        context_switches: switches,
        interruptions,
        distracted_ratio: if active > 0 { distracted as f64 / active as f64 } else { 0.0 },
        totals: category_totals(&normalized),
        grid: grid(&segs, cats),
        distractions: distractions(&segs),
        longest_focus: longest(&blocks),
        focus_blocks: blocks,
        programs: Vec::new(),
    }
}

/// Foreground-program time from screenshot metadata rows `(captured_at, app_name)`, sorted by
/// time. Each row stands for the time until the next row, capped at two minutes; the last row
/// counts `default_secs`. Rows taken while away or with a private app in front are excluded by
/// the query.
pub fn program_usage(rows: &[(i64, Option<String>)], default_secs: i64, limit: usize) -> Vec<NamedSecs> {
    let mut out: Vec<(String, String, i64)> = Vec::new(); // key, display name, secs
    for (i, (ts, app)) in rows.iter().enumerate() {
        let Some(app) = app.as_deref().map(str::trim).filter(|a| !a.is_empty()) else { continue };
        let dur = match rows.get(i + 1) {
            Some((next, _)) => (next - ts).clamp(0, 120),
            None => default_secs.clamp(0, 120),
        };
        let key = app.to_lowercase();
        match out.iter_mut().find(|(k, _, _)| *k == key) {
            Some((_, _, s)) => *s += dur,
            None => out.push((key, app.to_string(), dur)),
        }
    }
    out.retain(|(_, _, s)| *s > 0);
    out.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.cmp(&b.1)));
    out.into_iter().take(limit).map(|(_, name, secs)| NamedSecs { name, secs }).collect()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::types::default_categories;
    use chrono::{Local, TimeZone};

    pub fn at(day: (i32, u32, u32), h: u32, m: u32) -> i64 {
        Local.with_ymd_and_hms(day.0, day.1, day.2, h, m, 0).unwrap().timestamp()
    }

    pub fn card(id: i64, start: i64, end: i64, category: &str, title: &str) -> StoredCard {
        StoredCard {
            id,
            batch_id: None,
            start: timeutil::fmt_clock(start),
            end: timeutil::fmt_clock(end),
            start_ts: start,
            end_ts: end,
            day: timeutil::logical_day(start),
            title: title.into(),
            summary: format!("{title} summary"),
            category: category.into(),
            subcategory: String::new(),
            detailed_summary: String::new(),
            distractions: vec![],
            app_sites: None,
            is_idle_shortcut: false,
        }
    }

    pub fn distraction(start: i64, end: i64, title: &str) -> Distraction {
        Distraction {
            start_time: timeutil::fmt_clock(start),
            end_time: timeutil::fmt_clock(end),
            title: title.into(),
            summary: format!("{title} details"),
        }
    }

    const D: (i32, u32, u32) = (2026, 10, 14);

    fn sample() -> Vec<StoredCard> {
        let mut work = card(1, at(D, 9, 0), at(D, 10, 30), "Work", "Pricing page");
        work.distractions = vec![distraction(at(D, 9, 40), at(D, 9, 45), "Checked X")];
        vec![
            work,
            card(2, at(D, 10, 32), at(D, 11, 0), "work", "Release notes"),
            card(3, at(D, 11, 0), at(D, 11, 20), "Distraction", "YouTube"),
            card(4, at(D, 11, 20), at(D, 12, 0), "Idle", "Idle"),
            card(5, at(D, 12, 0), at(D, 12, 15), "System", "Processing failed"),
            card(6, at(D, 13, 0), at(D, 14, 0), "Personal", "Bills"),
            // overlaps card 6 by 10 minutes
            card(7, at(D, 13, 50), at(D, 14, 10), "Work", "Email"),
        ]
    }

    #[test]
    fn day_totals_and_chips() {
        let (s, e) = timeutil::logical_day_bounds("2026-10-14").unwrap();
        let cards = sample();
        let st = compute_day("2026-10-14", &cards, &default_categories(), s, e);
        let names: Vec<(&str, i64)> = st.totals.iter().map(|t| (t.name.as_str(), t.secs / 60)).collect();
        // Work: 90 + 28 + 10 (card 7 after the overlap) ; Personal 60 ; Idle 40 ; Distraction 20
        assert_eq!(names, vec![("Work", 128), ("Personal", 60), ("Idle", 40), ("Distraction", 20)]);
        assert_eq!(st.captured_secs / 60, 248);
        assert_eq!(st.idle_secs / 60, 40);
        // 5 minutes noted inside the Work card move from focus to distracted
        assert_eq!(st.distracted_secs / 60, 25);
        assert_eq!(st.focus_secs / 60, 128 + 60 - 5);
        assert_eq!(st.interruptions, 1);
        // Work → Distraction → Idle → Personal → Work
        assert_eq!(st.context_switches, 4);
        // gaps: 10:30-10:32 and 12:00-13:00 (the System card is ignored)
        assert_eq!(st.transition_secs / 60, 62);
        assert_eq!(st.card_count, 6);
        // Percentages use active time: 248 recorded minus 40 idle.
        assert_eq!(st.active_secs / 60, 208);
        assert!((st.distracted_ratio - 25.0 / 208.0).abs() < 1e-9);
    }

    #[test]
    fn counts_as_choice_moves_time_between_focus_distraction_and_neither() {
        let (s, e) = timeutil::logical_day_bounds("2026-10-14").unwrap();
        let mut cats = default_categories();
        // Personal counts as neither, and a renamed distraction category keeps counting.
        cats[1].counts_as = Some(CountsAs::Neither);
        cats[2].name = "Scrolling".into();
        let mut cards = sample();
        cards[2].category = "Scrolling".into();
        let st = compute_day("2026-10-14", &cards, &cats, s, e);
        // Focus: Work 128 minus the 5 noted minutes; Personal (60) is neither.
        assert_eq!(st.focus_secs / 60, 123);
        assert_eq!(st.distracted_secs / 60, 25);
        // Active time still counts Personal: only idle is left out.
        assert_eq!(st.active_secs / 60, 208);
        let personal = st.totals.iter().find(|t| t.name == "Personal").unwrap();
        assert_eq!(personal.kind, Kind::Neither);
        // Focus blocks only take focus categories: Personal 13:00-14:00 is no longer one, the
        // Work card after it (trimmed to 14:00-14:10 by the overlap) is.
        let blocks: Vec<(i64, i64)> = st.focus_blocks.iter().map(|b| (b.start_ts, b.end_ts)).collect();
        assert_eq!(blocks, vec![(at(D, 9, 0), at(D, 11, 0)), (at(D, 14, 0), at(D, 14, 10))]);
        assert_eq!(counts_as("scrolling", &cats), CountsAs::Distraction);
        assert_eq!(counts_as("Gaming", &cats), CountsAs::Focus, "unknown names follow the name rule");
        assert_eq!(counts_as("System", &cats), CountsAs::Neither);
        assert_eq!(counts_as("idle", &cats), CountsAs::Neither);
        // daytrace-mcp's weekly focus minutes follow the same choice.
        let totals = vec![("Work".to_string(), 120), ("Personal".to_string(), 60), ("Scrolling".to_string(), 30), ("Idle".to_string(), 20), ("Gaming".to_string(), 10)];
        assert_eq!(focus_minutes(&totals, &cats), 130, "Work and the unknown Gaming; Personal is neither");
    }

    #[test]
    fn focus_blocks_merge_short_gaps() {
        let (s, e) = timeutil::logical_day_bounds("2026-10-14").unwrap();
        let st = compute_day("2026-10-14", &sample(), &default_categories(), s, e);
        // 9:00-11:00 (2-minute gap merged), 13:00-14:10 (Personal + Work)
        let blocks: Vec<(i64, i64)> = st.focus_blocks.iter().map(|b| (b.start_ts, b.end_ts)).collect();
        assert_eq!(blocks, vec![(at(D, 9, 0), at(D, 11, 0)), (at(D, 13, 0), at(D, 14, 10))]);
        assert_eq!(st.longest_focus.unwrap().secs, 120 * 60);
    }

    #[test]
    fn distractions_list_both_sources() {
        let (s, e) = timeutil::logical_day_bounds("2026-10-14").unwrap();
        let st = compute_day("2026-10-14", &sample(), &default_categories(), s, e);
        let d: Vec<(&str, Option<&str>, i64)> =
            st.distractions.iter().map(|d| (d.title.as_str(), d.within_card.as_deref(), d.secs / 60)).collect();
        assert_eq!(d, vec![("Checked X", Some("Pricing page"), 5), ("YouTube", None, 20)]);
    }

    #[test]
    fn grid_window_rows_and_occupancy() {
        let (s, e) = timeutil::logical_day_bounds("2026-10-14").unwrap();
        let st = compute_day("2026-10-14", &sample(), &default_categories(), s, e);
        let g = &st.grid;
        // first card at 9:00 → 5 h after 4 AM; at least 12 hours wide
        assert_eq!(g.start_minute, 300.0);
        assert_eq!(g.end_minute, 300.0 + 720.0);
        assert_eq!(g.rows.len(), 4, "System has no row");
        let names: Vec<&str> = g.rows.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["Work", "Personal", "Distraction", "Idle"]);
        let work = &g.rows[0];
        assert_eq!(work.cells.len(), 48);
        assert_eq!(work.cells[0].occupancy, 1.0);
        assert_eq!(work.cells[0].title.as_deref(), Some("Pricing page"));
        // 10:30-10:45 slot: 10:32-10:45 of Release notes = 13/15
        assert!((work.cells[6].occupancy - 13.0 / 15.0).abs() < 1e-9);
        assert_eq!(work.cells[6].card_id, Some(2));
        // 11:00-11:15 is Distraction (row 2)
        assert_eq!(g.dominant[8], Some(2));
        assert_eq!(g.dominant[12], None, "12:00-12:15 only has the System card");
        // markers: 9:40-9:45 and 11:00-11:20 stay separate
        assert_eq!(g.markers.len(), 2);
        assert_eq!((g.markers[0].start_minute, g.markers[0].end_minute), (340.0, 345.0));
    }

    #[test]
    fn markers_merge_within_two_minutes() {
        let (s, e) = timeutil::logical_day_bounds("2026-10-14").unwrap();
        let mut c = card(1, at(D, 9, 0), at(D, 10, 0), "Work", "Spec");
        c.distractions = vec![
            distraction(at(D, 9, 10), at(D, 9, 12), "X"),
            distraction(at(D, 9, 13), at(D, 9, 15), "Reddit"),
            distraction(at(D, 9, 30), at(D, 9, 31), "X"),
        ];
        let st = compute_day("2026-10-14", &[c], &default_categories(), s, e);
        let m: Vec<(f64, f64, &str)> = st.grid.markers.iter().map(|m| (m.start_minute, m.end_minute, m.title.as_str())).collect();
        assert_eq!(m, vec![(310.0, 315.0, "X, Reddit"), (330.0, 331.0, "X")]);
    }

    #[test]
    fn empty_day_uses_placeholder_window() {
        let (s, e) = timeutil::logical_day_bounds("2026-10-14").unwrap();
        let st = compute_day("2026-10-14", &[], &default_categories(), s, e);
        assert_eq!((st.grid.start_minute, st.grid.end_minute), (300.0, 1020.0));
        assert!(st.totals.is_empty() && st.longest_focus.is_none());
        assert_eq!(st.distracted_ratio, 0.0);
    }

    #[test]
    fn cards_are_clipped_to_the_logical_day() {
        let (s, e) = timeutil::logical_day_bounds("2026-10-14").unwrap();
        // 3:30-4:30 AM on the 15th: only 30 minutes belong to the 14th
        let c = card(1, at((2026, 10, 15), 3, 30), at((2026, 10, 15), 4, 30), "Work", "Late");
        let st = compute_day("2026-10-14", &[c], &default_categories(), s, e);
        assert_eq!(st.captured_secs, 30 * 60);
        assert_eq!(st.grid.end_minute, 1440.0);
    }

    #[test]
    fn distraction_outside_card_collapses_to_a_minute() {
        let c = card(1, at(D, 9, 0), at(D, 10, 0), "Work", "Spec");
        let d = distraction(at(D, 11, 0), at(D, 11, 5), "Late");
        assert_eq!(distraction_span(&c, &d), Some((at(D, 9, 59), at(D, 10, 0))));
    }

    #[test]
    fn unknown_category_gets_fallback_color() {
        let info = cat_info("Gaming", &default_categories());
        assert_eq!(info.kind, Kind::Focus);
        assert!(info.color.starts_with('#') && info.color.len() == 7);
        assert_eq!(cat_info(" distraction ", &[]).kind, Kind::Distraction);
        assert_eq!(cat_info("", &[]).name, "Uncategorized");
    }

    #[test]
    fn programs_from_screenshot_rows() {
        let rows = vec![
            (0, Some("Code".to_string())),
            (10, Some("code".to_string())),
            (20, Some("chrome".to_string())),
            (30, None),
            (40, Some("chrome".to_string())),
            (1000, Some("Code".to_string())),
        ];
        let p = program_usage(&rows, 10, 5);
        // chrome: 10 + 120 (capped); Code: 10 + 10 + 10 (last row)
        assert_eq!(p, vec![NamedSecs { name: "chrome".into(), secs: 130 }, NamedSecs { name: "Code".into(), secs: 30 }]);
    }
}
