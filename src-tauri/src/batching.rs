//! Batch formation and the idle shortcut, ported from Dayflow's AnalysisManager and
//! IdleBatchClassifier with the same constants.

use crate::db::Shot;

pub const TARGET_DURATION: i64 = 900; // 15 min
pub const MAX_GAP: i64 = 120;
pub const CARD_LOOKBACK: i64 = 2700; // 45 min
pub const MIN_BATCH_SPAN: i64 = 300;

/// Group unbatched screenshots into buckets. A bucket closes when the next shot comes more than
/// MAX_GAP after the previous one or would stretch the bucket past TARGET_DURATION. The trailing
/// bucket is held back until it spans the full target duration.
pub fn bucketize(shots: &[Shot]) -> Vec<Vec<Shot>> {
    let mut buckets: Vec<Vec<Shot>> = Vec::new();
    let mut cur: Vec<Shot> = Vec::new();
    for s in shots {
        if let (Some(first), Some(prev)) = (cur.first(), cur.last()) {
            let gap = s.captured_at - prev.captured_at;
            let span = s.captured_at - first.captured_at;
            if gap > MAX_GAP || span > TARGET_DURATION {
                buckets.push(std::mem::take(&mut cur));
            }
        }
        cur.push(s.clone());
    }
    if let (Some(first), Some(last)) = (cur.first(), cur.last()) {
        if last.captured_at - first.captured_at >= TARGET_DURATION {
            buckets.push(cur);
        }
    }
    buckets
}

#[derive(Debug, Clone, PartialEq)]
pub struct IdleStats {
    pub coverage: f64,
    pub long_idle_share: f64,
    pub sample_share: f64,
    pub largest_gap: i64,
}

/// Dayflow's rules: a batch is idle only if it spans at least 12 minutes, the union of idle
/// intervals covers ≥95% of it, ≥90% of shots report ≥60 s idle, ≥90% of shots carry an idle
/// sample, and no uncovered gap exceeds 30 s.
pub fn classify_idle(shots: &[Shot], batch_start: i64, batch_end: i64) -> Option<IdleStats> {
    let span = batch_end - batch_start;
    if span < 720 || shots.is_empty() {
        return None;
    }
    let samples: Vec<&Shot> = shots.iter().filter(|s| s.idle_seconds > 0).collect();
    if samples.is_empty() {
        return None;
    }
    let mut intervals: Vec<(i64, i64)> = samples
        .iter()
        .map(|s| ((s.captured_at - s.idle_seconds).max(batch_start), s.captured_at.min(batch_end)))
        .filter(|(a, b)| b > a)
        .collect();
    intervals.sort();
    let mut merged: Vec<(i64, i64)> = Vec::new();
    for (a, b) in intervals {
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    let covered: i64 = merged.iter().map(|(a, b)| b - a).sum();
    let coverage = covered as f64 / span as f64;

    let mut largest_gap = 0;
    let mut cursor = batch_start;
    for (a, b) in &merged {
        largest_gap = largest_gap.max(a - cursor);
        cursor = cursor.max(*b);
    }
    largest_gap = largest_gap.max(batch_end - cursor);

    let n = shots.len() as f64;
    let long_idle_share = shots.iter().filter(|s| s.idle_seconds >= 60).count() as f64 / n;
    let sample_share = samples.len() as f64 / n;

    let stats = IdleStats { coverage, long_idle_share, sample_share, largest_gap };
    if coverage >= 0.95 && long_idle_share >= 0.90 && sample_share >= 0.90 && largest_gap <= 30 {
        Some(stats)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shot(id: i64, t: i64, idle: i64) -> Shot {
        Shot { id, captured_at: t, file_path: String::new(), idle_seconds: idle, app_name: None, window_title: None, hidden_app: None, skip_reason: None }
    }

    #[test]
    fn holds_back_short_trailing_bucket() {
        let shots: Vec<Shot> = (0..60).map(|i| shot(i, i * 10, 0)).collect(); // 0..590 s
        assert!(bucketize(&shots).is_empty());
    }

    #[test]
    fn splits_on_span_and_gap() {
        let mut shots: Vec<Shot> = (0..=95).map(|i| shot(i, i * 10, 0)).collect(); // 0..950
        // gap of 300 s, then a short tail that must be held back
        shots.extend((0..10).map(|i| shot(200 + i, 1250 + i * 10, 0)));
        let b = bucketize(&shots);
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].first().unwrap().captured_at, 0);
        assert_eq!(b[0].last().unwrap().captured_at, 900);
        assert_eq!(b[1].first().unwrap().captured_at, 910);
        assert_eq!(b[1].last().unwrap().captured_at, 950);
    }

    #[test]
    fn idle_classifier() {
        // user idle the whole time: idle grows with time
        let idle: Vec<Shot> = (0..=90).map(|i| shot(i, i * 10, 600 + i * 10)).collect();
        assert!(classify_idle(&idle, 0, 900).is_some());
        // active user
        let active: Vec<Shot> = (0..=90).map(|i| shot(i, i * 10, 2)).collect();
        assert!(classify_idle(&active, 0, 900).is_none());
        // too short
        assert!(classify_idle(&idle[..60], 0, 600).is_none());
    }
}
