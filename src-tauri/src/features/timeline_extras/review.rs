//! Timeline review: rate stretches of the day Focused / Neutral / Distracted. Ported from Dayflow
//! (MIT, (c) 2025 Jerry Liu): Core/Recording/StorageManager+TimelineReview.swift (ratings are time
//! segments, so they survive card rewrites; a new rating cuts the overlapped part out of older
//! ones), Views/Components/DaySummaryStats.makeReviewSummary and fetchUnreviewedTimelineCardCount
//! (a card counts as reviewed when 80 % of it is rated). Everything stays on this PC.

use crate::db::{Db, StoredCard, SYSTEM_CATEGORY};
use anyhow::{bail, Result};
use rusqlite::params;
use serde::Serialize;

pub const RATINGS: [&str; 3] = ["focused", "neutral", "distracted"];
/// Dayflow's coverageThreshold for "reviewed".
pub const REVIEWED_COVERAGE: f64 = 0.8;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub id: i64,
    pub start_ts: i64,
    pub end_ts: i64,
    pub rating: String,
}

/// The parts of `existing` that survive a new rating over [start, end): ids to delete and the
/// fragments to re-insert (Dayflow applyReviewRating).
pub fn cut(existing: &[Segment], start: i64, end: i64) -> (Vec<i64>, Vec<(i64, i64, String)>) {
    let mut delete = Vec::new();
    let mut keep = Vec::new();
    for s in existing.iter().filter(|s| !(s.end_ts <= start || s.start_ts >= end)) {
        delete.push(s.id);
        if s.start_ts < start {
            let e = start.min(s.end_ts);
            if e > s.start_ts {
                keep.push((s.start_ts, e, s.rating.clone()));
            }
        }
        if s.end_ts > end {
            let b = end.max(s.start_ts);
            if s.end_ts > b {
                keep.push((b, s.end_ts, s.rating.clone()));
            }
        }
    }
    (delete, keep)
}

pub fn segments_overlapping(db: &Db, start: i64, end: i64) -> Result<Vec<Segment>> {
    if end <= start {
        return Ok(Vec::new());
    }
    let c = db.conn();
    let mut st = c.prepare(
        "SELECT id, start_ts, end_ts, rating FROM timeline_review_ratings
         WHERE NOT (end_ts <= ?1 OR start_ts >= ?2) ORDER BY start_ts ASC",
    )?;
    let rows = st
        .query_map([start, end], |r| Ok(Segment { id: r.get(0)?, start_ts: r.get(1)?, end_ts: r.get(2)?, rating: r.get(3)? }))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Rates [start, end). `None` removes any rating there.
pub fn apply(db: &Db, start: i64, end: i64, rating: Option<&str>) -> Result<()> {
    if end <= start {
        return Ok(());
    }
    if let Some(r) = rating {
        if !RATINGS.contains(&r) {
            bail!("Unknown rating: {r}");
        }
    }
    let existing = segments_overlapping(db, start, end)?;
    let (delete, keep) = cut(&existing, start, end);
    let mut c = db.conn();
    let tx = c.transaction()?;
    for id in delete {
        tx.execute("DELETE FROM timeline_review_ratings WHERE id = ?", [id])?;
    }
    for (s, e, r) in keep {
        tx.execute("INSERT INTO timeline_review_ratings (start_ts, end_ts, rating) VALUES (?, ?, ?)", params![s, e, r])?;
    }
    if let Some(r) = rating {
        tx.execute("INSERT INTO timeline_review_ratings (start_ts, end_ts, rating) VALUES (?, ?, ?)", params![start, end, r])?;
    }
    tx.commit()?;
    Ok(())
}

/// Seconds of each rating inside [start, end).
fn per_rating(start: i64, end: i64, segs: &[Segment]) -> [i64; 3] {
    let mut out = [0i64; 3];
    for s in segs {
        let (a, b) = (s.start_ts.max(start), s.end_ts.min(end));
        if b > a {
            if let Some(i) = RATINGS.iter().position(|r| *r == s.rating) {
                out[i] += b - a;
            }
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardReview {
    pub card_id: i64,
    /// The rating covering most of the card.
    pub rating: String,
    /// Share of the card that is rated (0..1).
    pub coverage: f64,
}

/// The rating of a card, if any part of it is rated.
pub fn card_review(card: &StoredCard, segs: &[Segment]) -> Option<CardReview> {
    let d = card.end_ts - card.start_ts;
    if d <= 0 {
        return None;
    }
    let secs = per_rating(card.start_ts, card.end_ts, segs);
    let total: i64 = secs.iter().sum();
    if total == 0 {
        return None;
    }
    let best = (0..3).max_by_key(|i| (secs[*i], -(*i as i64))).unwrap_or(0);
    Some(CardReview { card_id: card.id, rating: RATINGS[best].to_string(), coverage: (total as f64 / d as f64).min(1.0) })
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewSummary {
    pub focused_secs: i64,
    pub neutral_secs: i64,
    pub distracted_secs: i64,
    /// End of the latest rated stretch (Dayflow's "last reviewed").
    pub last_reviewed_at: Option<i64>,
    /// Cards of the day with less than 80 % rated.
    pub unreviewed_cards: usize,
    /// First unreviewed card, to jump to.
    pub first_unreviewed: Option<i64>,
}

/// Merged rated coverage clipped to the day (Dayflow mergeCoverageSegments).
pub fn merge_coverage(segs: &[Segment], day_start: i64, day_end: i64) -> Vec<(i64, i64)> {
    let mut v: Vec<(i64, i64)> =
        segs.iter().map(|s| (s.start_ts.max(day_start), s.end_ts.min(day_end))).filter(|(a, b)| b > a).collect();
    v.sort();
    let mut out: Vec<(i64, i64)> = Vec::new();
    for (a, b) in v {
        match out.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => out.push((a, b)),
        }
    }
    out
}

fn covered(start: i64, end: i64, merged: &[(i64, i64)]) -> i64 {
    merged.iter().map(|(a, b)| (end.min(*b) - start.max(*a)).max(0)).sum()
}

/// Dayflow makeReviewSummary + fetchUnreviewedTimelineCardCount for one day. `cards` are the
/// day's live cards; System cards are left out.
pub fn summary(cards: &[StoredCard], segs: &[Segment], day_start: i64, day_end: i64) -> ReviewSummary {
    let secs = per_rating(day_start, day_end, segs);
    let last = segs.iter().filter(|s| s.end_ts > day_start && s.start_ts < day_end).map(|s| s.end_ts.min(day_end)).max();
    let merged = merge_coverage(segs, day_start, day_end);
    let mut unreviewed = 0;
    let mut first = None;
    for c in cards.iter().filter(|c| c.category.trim() != SYSTEM_CATEGORY && c.start_ts >= day_start && c.start_ts < day_end) {
        let d = c.end_ts - c.start_ts;
        let ok = d > 0 && covered(c.start_ts, c.end_ts, &merged) as f64 / d as f64 >= REVIEWED_COVERAGE;
        if !ok {
            unreviewed += 1;
            first = first.or(Some(c.id));
        }
    }
    ReviewSummary {
        focused_secs: secs[0],
        neutral_secs: secs[1],
        distracted_secs: secs[2],
        last_reviewed_at: last,
        unreviewed_cards: unreviewed,
        first_unreviewed: first,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(id: i64, s: i64, e: i64, r: &str) -> Segment {
        Segment { id, start_ts: s, end_ts: e, rating: r.into() }
    }

    #[test]
    fn cut_keeps_the_parts_outside_the_new_rating() {
        let existing = [seg(1, 0, 100, "focused"), seg(2, 150, 200, "neutral"), seg(3, 300, 400, "distracted")];
        let (del, keep) = cut(&existing, 50, 170);
        assert_eq!(del, vec![1, 2]);
        assert_eq!(keep, vec![(0, 50, "focused".to_string()), (170, 200, "neutral".to_string())]);
        // A rating inside an older one splits it in two.
        let (del, keep) = cut(&existing, 320, 330);
        assert_eq!(del, vec![3]);
        assert_eq!(keep, vec![(300, 320, "distracted".to_string()), (330, 400, "distracted".to_string())]);
    }

    #[test]
    fn ratings_are_stored_as_segments_and_survive_card_ids() {
        let db = Db::memory();
        apply(&db, 1000, 2000, Some("focused")).unwrap();
        apply(&db, 1500, 2500, Some("distracted")).unwrap();
        let segs = segments_overlapping(&db, 0, 10_000).unwrap();
        let spans: Vec<_> = segs.iter().map(|s| (s.start_ts, s.end_ts, s.rating.as_str())).collect();
        assert_eq!(spans, vec![(1000, 1500, "focused"), (1500, 2500, "distracted")]);
        assert!(apply(&db, 0, 10, Some("great")).is_err());
        // Clearing removes only the cleared part.
        apply(&db, 1400, 1600, None).unwrap();
        let segs = segments_overlapping(&db, 0, 10_000).unwrap();
        let spans: Vec<_> = segs.iter().map(|s| (s.start_ts, s.end_ts)).collect();
        assert_eq!(spans, vec![(1000, 1400), (1600, 2500)]);
    }

    #[test]
    fn card_review_and_day_summary() {
        let a = crate::features::insights::stats::tests::card(1, 1000, 2000, "Work", "A");
        let b = crate::features::insights::stats::tests::card(2, 2000, 3000, "Work", "B");
        let failed = crate::features::insights::stats::tests::card(3, 3000, 3600, "System", "Processing failed");
        let segs = [seg(1, 1000, 1900, "focused"), seg(2, 2000, 2200, "distracted")];
        let r = card_review(&a, &segs).unwrap();
        assert_eq!((r.rating.as_str(), r.coverage), ("focused", 0.9));
        assert_eq!(card_review(&b, &segs).unwrap().rating, "distracted");
        let s = summary(&[a, b, failed], &segs, 0, 10_000);
        assert_eq!((s.focused_secs, s.neutral_secs, s.distracted_secs), (900, 0, 200));
        assert_eq!(s.last_reviewed_at, Some(2200));
        assert_eq!((s.unreviewed_cards, s.first_unreviewed), (1, Some(2)), "A is 90 % rated, B 20 %, failed card ignored");
    }
}
