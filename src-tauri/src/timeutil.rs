//! Clock-string helpers. Dayflow exchanges card times with the LLM as `h:mm AM/PM` strings in
//! local time (no date, no seconds) and rebuilds Unix timestamps by picking the day closest to
//! the analysis window's midpoint. Logical days start at 4 AM.

use chrono::{DateTime, Duration, Local, NaiveDate, NaiveTime, TimeZone, Timelike};

pub const DAY_START_HOUR: u32 = 4;

pub fn now_ts() -> i64 {
    Local::now().timestamp()
}

pub fn local(ts: i64) -> DateTime<Local> {
    Local
        .timestamp_opt(ts, 0)
        .single()
        .unwrap_or_else(|| Local.timestamp_opt(0, 0).unwrap())
}

/// `h:mm AM` in local time, e.g. `2:30 PM`.
pub fn fmt_clock(ts: i64) -> String {
    local(ts).format("%-I:%M %p").to_string()
}

/// Lenient parser for `h:mm AM/PM` (also accepts `2:30pm`, `02:30 PM`, `14:30`).
pub fn parse_clock(s: &str) -> Option<NaiveTime> {
    let t = s.trim().to_ascii_uppercase().replace('.', "");
    let (body, pm) = if let Some(b) = t.strip_suffix("PM") {
        (b.trim().to_string(), Some(true))
    } else if let Some(b) = t.strip_suffix("AM") {
        (b.trim().to_string(), Some(false))
    } else {
        (t.clone(), None)
    };
    let mut parts = body.split(':');
    let h: u32 = parts.next()?.trim().parse().ok()?;
    let m: u32 = parts.next()?.trim().parse().ok()?;
    // seconds, if present, are ignored
    let h24 = match pm {
        Some(true) => {
            if !(1..=12).contains(&h) {
                return None;
            }
            if h == 12 { 12 } else { h + 12 }
        }
        Some(false) => {
            if !(1..=12).contains(&h) {
                return None;
            }
            if h == 12 { 0 } else { h }
        }
        None => h,
    };
    NaiveTime::from_hms_opt(h24, m, 0)
}

/// Minutes since midnight for a clock string; 0 if unparsable (mirrors Dayflow's timeToMinutes).
pub fn clock_minutes(s: &str) -> f64 {
    parse_clock(s)
        .map(|t| (t.hour() * 60 + t.minute()) as f64)
        .unwrap_or(0.0)
}

/// Duration in minutes between two clock strings, wrapping past midnight.
pub fn clock_span_minutes(start: &str, end: &str) -> Option<f64> {
    let s = parse_clock(start)?;
    let e = parse_clock(end)?;
    let mut d = (e.hour() * 60 + e.minute()) as f64 - (s.hour() * 60 + s.minute()) as f64;
    if d < 0.0 {
        d += 24.0 * 60.0;
    }
    Some(d)
}

fn local_at(date: NaiveDate, t: NaiveTime) -> Option<i64> {
    let ndt = date.and_time(t);
    // earliest() handles DST folds; gaps return None
    Local.from_local_datetime(&ndt).earliest().map(|d| d.timestamp())
}

/// Resolve a card's clock strings to Unix seconds, choosing the calendar day (previous, same or
/// next relative to the anchor) whose start time lies closest to `anchor_ts`. End is placed on the
/// start's day and moved forward one day if it falls before the start.
pub fn resolve_card_range(start: &str, end: &str, anchor_ts: i64) -> Option<(i64, i64)> {
    let st = parse_clock(start)?;
    let et = parse_clock(end)?;
    let anchor_date = local(anchor_ts).date_naive();
    let mut best: Option<(i64, NaiveDate)> = None;
    for offset in [-1i64, 0, 1] {
        let date = anchor_date + Duration::days(offset);
        if let Some(ts) = local_at(date, st) {
            let better = match best {
                None => true,
                Some((b, _)) => (ts - anchor_ts).abs() < (b - anchor_ts).abs(),
            };
            if better {
                best = Some((ts, date));
            }
        }
    }
    let (start_ts, date) = best?;
    let mut end_ts = local_at(date, et)?;
    if end_ts < start_ts {
        end_ts = local_at(date + Duration::days(1), et)?;
    }
    Some((start_ts, end_ts))
}

/// Logical day (`YYYY-MM-DD`) of a timestamp: times before 4 AM belong to the previous day.
pub fn logical_day(ts: i64) -> String {
    let shifted = local(ts) - Duration::hours(DAY_START_HOUR as i64);
    shifted.date_naive().format("%Y-%m-%d").to_string()
}

/// Unix range [start, end) of a logical day.
pub fn logical_day_bounds(day: &str) -> Option<(i64, i64)> {
    let date = NaiveDate::parse_from_str(day, "%Y-%m-%d").ok()?;
    let t = NaiveTime::from_hms_opt(DAY_START_HOUR, 0, 0)?;
    let start = local_at(date, t)?;
    let end = local_at(date + Duration::days(1), t)?;
    Some((start, end))
}

/// `MM:SS` or `HH:MM:SS` → seconds; unparsable → 0 (mirrors Dayflow's parseVideoTimestamp).
pub fn parse_elapsed(s: &str) -> i64 {
    let parts: Vec<i64> = s
        .trim()
        .split(':')
        .map(|p| p.trim().parse::<f64>().map(|f| f as i64))
        .collect::<Result<_, _>>()
        .unwrap_or_default();
    match parts.as_slice() {
        [h, m, sec] => h * 3600 + m * 60 + sec,
        [m, sec] => m * 60 + sec,
        _ => 0,
    }
}

pub fn fmt_hhmmss(secs: i64) -> String {
    let s = secs.max(0);
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

pub fn fmt_mmss(secs: i64) -> String {
    let s = secs.max(0);
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// `h:mm AM` from minutes-of-day (wraps past 24h).
pub fn minutes_to_clock(minutes: f64) -> String {
    let total = minutes.max(0.0) as i64;
    let h = (total / 60) % 24;
    let m = total % 60;
    let period = if h < 12 { "AM" } else { "PM" };
    let mut dh = h % 12;
    if dh == 0 {
        dh = 12;
    }
    format!("{}:{:02} {}", dh, m, period)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clock_variants() {
        assert_eq!(parse_clock("2:30 PM"), NaiveTime::from_hms_opt(14, 30, 0));
        assert_eq!(parse_clock("12:05 AM"), NaiveTime::from_hms_opt(0, 5, 0));
        assert_eq!(parse_clock("12:05 PM"), NaiveTime::from_hms_opt(12, 5, 0));
        assert_eq!(parse_clock("2:30pm"), NaiveTime::from_hms_opt(14, 30, 0));
        assert_eq!(parse_clock("14:30"), NaiveTime::from_hms_opt(14, 30, 0));
        assert_eq!(parse_clock("13:30 PM"), None);
        assert_eq!(parse_clock("garbage"), None);
    }

    #[test]
    fn round_trips_clock() {
        let ts = Local.with_ymd_and_hms(2026, 10, 16, 14, 7, 42).unwrap().timestamp();
        assert_eq!(fmt_clock(ts), "2:07 PM");
        let (s, e) = resolve_card_range("2:07 PM", "2:30 PM", ts).unwrap();
        assert_eq!(s, ts - 42);
        assert_eq!(e - s, 23 * 60);
    }

    #[test]
    fn resolves_across_midnight() {
        let anchor = Local.with_ymd_and_hms(2026, 10, 17, 0, 10, 0).unwrap().timestamp();
        let (s, e) = resolve_card_range("11:50 PM", "12:20 AM", anchor).unwrap();
        assert_eq!(local(s).format("%Y-%m-%d %H:%M").to_string(), "2026-10-16 23:50");
        assert_eq!(e - s, 30 * 60);
    }

    #[test]
    fn logical_day_starts_at_4am() {
        let early = Local.with_ymd_and_hms(2026, 10, 17, 3, 59, 0).unwrap().timestamp();
        let later = Local.with_ymd_and_hms(2026, 10, 17, 4, 0, 0).unwrap().timestamp();
        assert_eq!(logical_day(early), "2026-10-16");
        assert_eq!(logical_day(later), "2026-10-17");
        let (a, b) = logical_day_bounds("2026-10-17").unwrap();
        assert_eq!(a, later);
        assert!(b - a >= 23 * 3600);
    }

    #[test]
    fn elapsed_parsing() {
        assert_eq!(parse_elapsed("01:30"), 90);
        assert_eq!(parse_elapsed("00:14:59"), 899);
        assert_eq!(parse_elapsed("x"), 0);
        assert_eq!(fmt_hhmmss(899), "00:14:59");
        assert_eq!(fmt_mmss(899), "14:59");
        assert_eq!(minutes_to_clock(14.0 * 60.0 + 5.0), "2:05 PM");
        assert_eq!(clock_span_minutes("11:50 PM", "12:20 AM"), Some(30.0));
    }
}
