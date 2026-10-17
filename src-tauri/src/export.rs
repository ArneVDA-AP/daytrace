//! Markdown export of a range of days and the plain-text "Copy timeline" of one day. The layout
//! follows Dayflow's Utilities/TimelineClipboardFormatter.swift (MIT, (c) 2025 Jerry Liu):
//! a "<app> timeline · <day>" heading, numbered cards with time range and title, the category, the
//! summary and the details, days separated by `---`. Daytrace adds the subcategory, writes the
//! details as the step-by-step list and lists the interruptions. Failed (System) cards are left out.

use crate::db::{Db, StoredCard, SYSTEM_CATEGORY};
use anyhow::{anyhow, Result};
use chrono::{Duration, NaiveDate};

const EMPTY_DAY: &str = "No timeline activities were recorded for this day.";

fn parse_day(s: &str, what: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| anyhow!("bad {what} date"))
}

/// "Today, Oct 17" for the current logical day, otherwise "Thursday, Oct 16".
fn day_label(day: NaiveDate, today: &str) -> String {
    if day.format("%Y-%m-%d").to_string() == today {
        format!("Today, {}", day.format("%b %-d"))
    } else {
        day.format("%A, %b %-d").to_string()
    }
}

fn header(day: NaiveDate, today: &str) -> String {
    format!("Daytrace timeline · {}", day_label(day, today))
}

fn lines(text: &str) -> Vec<String> {
    text.lines().map(|l| l.trim()).filter(|l| !l.is_empty()).map(String::from).collect()
}

/// Detailed-summary lines without their bullet marks (the step-by-step log).
fn steps(c: &StoredCard) -> Vec<String> {
    let summary = lines(&c.summary).join("\n");
    let details = lines(&c.detailed_summary);
    if details.is_empty() || details.join("\n") == summary {
        return Vec::new();
    }
    details.iter().map(|l| l.trim_start_matches(['-', '•', '*']).trim().to_string()).filter(|l| !l.is_empty()).collect()
}

fn meta(c: &StoredCard) -> String {
    [c.category.trim(), c.subcategory.trim()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" • ")
}

fn interruption(d: &crate::types::Distraction) -> String {
    let mut s = format!("{} – {}: {}", d.start_time.trim(), d.end_time.trim(), d.title.trim());
    if !d.summary.trim().is_empty() {
        s += &format!(" — {}", d.summary.trim());
    }
    s
}

fn exported_cards(db: &Db, day: &str) -> Result<Vec<StoredCard>> {
    Ok(db.cards_for_day(day)?.into_iter().filter(|c| c.category.trim() != SYSTEM_CATEGORY).collect())
}

/// One day as Markdown (Dayflow makeMarkdown).
pub fn day_markdown(day: NaiveDate, cards: &[StoredCard], today: &str) -> String {
    let head = format!("## {}", header(day, today));
    if cards.is_empty() {
        return format!("{head}\n\n_{EMPTY_DAY}_");
    }
    let entries: Vec<String> = cards
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let mut out = vec![format!("{}. **{} – {} — {}**", i + 1, c.start.trim(), c.end.trim(), c.title.trim())];
            let m = meta(c);
            if !m.is_empty() {
                out.push(format!("   - _{m}_"));
            }
            let summary = lines(&c.summary);
            match summary.len() {
                0 => {}
                1 => out.push(format!("   - Summary: {}", summary[0])),
                _ => {
                    out.push("   - Summary:".into());
                    out.extend(summary.iter().map(|l| format!("      {l}")));
                }
            }
            let st = steps(c);
            if !st.is_empty() {
                out.push("   - Step by step:".into());
                out.extend(st.iter().map(|l| format!("      - {l}")));
            }
            if !c.distractions.is_empty() {
                out.push("   - Interruptions:".into());
                out.extend(c.distractions.iter().map(|d| format!("      - {}", interruption(d))));
            }
            out.join("\n")
        })
        .collect();
    format!("{head}\n\n{}", entries.join("\n\n"))
}

/// One day as plain text for the clipboard (Dayflow makeClipboardText).
pub fn day_text(day: NaiveDate, cards: &[StoredCard], today: &str) -> String {
    let head = header(day, today);
    if cards.is_empty() {
        return format!("{head}\n\n{EMPTY_DAY}");
    }
    let block = |label: &str, ls: &[String]| -> String {
        if ls.len() == 1 {
            format!("   {label}: {}", ls[0])
        } else {
            std::iter::once(format!("   {label}:")).chain(ls.iter().map(|l| format!("      {l}"))).collect::<Vec<_>>().join("\n")
        }
    };
    let entries: Vec<String> = cards
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let mut out = vec![format!("{}. {} – {} — {}", i + 1, c.start.trim(), c.end.trim(), c.title.trim())];
            let m = meta(c);
            if !m.is_empty() {
                out.push(format!("   {m}"));
            }
            let summary = lines(&c.summary);
            if !summary.is_empty() {
                out.push(block("Summary", &summary));
            }
            let st = steps(c);
            if !st.is_empty() {
                out.push(block("Step by step", &st));
            }
            if !c.distractions.is_empty() {
                let ds: Vec<String> = c.distractions.iter().map(interruption).collect();
                out.push(block("Interruptions", &ds));
            }
            out.join("\n")
        })
        .collect();
    format!("{head}\n\n{}", entries.join("\n\n"))
}

/// Markdown for an inclusive range of logical days. Returns (markdown, card count).
pub fn markdown(db: &Db, from_day: &str, to_day: &str) -> Result<(String, usize)> {
    markdown_at(db, from_day, to_day, &crate::timeutil::logical_day(crate::timeutil::now_ts()))
}

pub fn markdown_at(db: &Db, from_day: &str, to_day: &str, today: &str) -> Result<(String, usize)> {
    let from = parse_day(from_day, "from")?;
    let to = parse_day(to_day, "to")?;
    let (from, to) = if from <= to { (from, to) } else { (to, from) };
    let mut sections = Vec::new();
    let mut count = 0;
    let mut day = from;
    while day <= to {
        let cards = exported_cards(db, &day.format("%Y-%m-%d").to_string())?;
        count += cards.len();
        sections.push(day_markdown(day, &cards, today));
        day += Duration::days(1);
    }
    Ok((sections.join("\n\n---\n\n") + "\n", count))
}

/// The plain-text timeline of one day.
pub fn clipboard_text(db: &Db, day: &str, today: &str) -> Result<String> {
    let d = parse_day(day, "day")?;
    Ok(day_text(d, &exported_cards(db, day)?, today))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{ResolvedCard, ERROR_TITLE};
    use crate::types::{ActivityCard, Distraction};
    use chrono::{Local, TimeZone};

    fn seed() -> Db {
        let db = Db::memory();
        let t = |h: u32, m: u32| Local.with_ymd_and_hms(2026, 10, 16, h, m, 0).unwrap().timestamp();
        db.insert_screenshot(t(9, 0), "", 0, 0, None, None, None, None).unwrap();
        let b = db.create_batch(&db.unbatched_screenshots(0).unwrap()).unwrap();
        let card = |s: i64, e: i64, title: &str, cat: &str, sub: &str, summary: &str, details: &str, d: Option<Vec<Distraction>>| ResolvedCard {
            start_ts: s,
            end_ts: e,
            card: ActivityCard {
                start_time: crate::timeutil::fmt_clock(s),
                end_time: crate::timeutil::fmt_clock(e),
                category: cat.into(),
                subcategory: sub.into(),
                title: title.into(),
                summary: summary.into(),
                detailed_summary: details.into(),
                distractions: d,
                app_sites: None,
            },
            idle_meta: None,
        };
        let cards = vec![
            card(
                t(9, 0),
                t(9, 30),
                "Export feature",
                "Work",
                "Coding",
                "Built the export.",
                "- 9:00 AM opened export.rs\n- 9:12 AM wrote tests",
                Some(vec![Distraction { start_time: "9:10 AM".into(), end_time: "9:12 AM".into(), title: "Chat".into(), summary: "Answered a message".into() }]),
            ),
            card(t(9, 30), t(9, 45), ERROR_TITLE, SYSTEM_CATEGORY, "Error", "x", "y", None),
            card(t(9, 45), t(10, 0), "Lunch", "Personal", "", "Ate.", "Ate.", None),
        ];
        db.replace_cards_in_range(t(9, 0), t(10, 0), &cards, b, true).unwrap();
        db
    }

    #[test]
    fn markdown_has_subcategory_steps_and_interruptions() {
        let db = seed();
        let (md, n) = markdown_at(&db, "2026-10-17", "2026-10-15", "2026-10-17").unwrap();
        assert_eq!(n, 2, "the failed card is left out");
        let expected_day = "## Daytrace timeline · Friday, Oct 16\n\n\
            1. **9:00 AM – 9:30 AM — Export feature**\n   - _Work • Coding_\n   - Summary: Built the export.\n   - Step by step:\n      - 9:00 AM opened export.rs\n      - 9:12 AM wrote tests\n   - Interruptions:\n      - 9:10 AM – 9:12 AM: Chat — Answered a message\n\n\
            2. **9:45 AM – 10:00 AM — Lunch**\n   - _Personal_\n   - Summary: Ate.";
        assert!(md.contains(expected_day), "{md}");
        assert!(md.starts_with("## Daytrace timeline · Thursday, Oct 15\n\n_No timeline activities were recorded for this day._\n\n---\n\n"), "{md}");
        assert!(md.contains("## Daytrace timeline · Today, Oct 17"));
        assert!(!md.contains(ERROR_TITLE));
    }

    #[test]
    fn clipboard_text_of_a_day() {
        let db = seed();
        let text = clipboard_text(&db, "2026-10-16", "2026-10-17").unwrap();
        assert_eq!(
            text,
            "Daytrace timeline · Friday, Oct 16\n\n\
             1. 9:00 AM – 9:30 AM — Export feature\n   Work • Coding\n   Summary: Built the export.\n   Step by step:\n      9:00 AM opened export.rs\n      9:12 AM wrote tests\n   Interruptions: 9:10 AM – 9:12 AM: Chat — Answered a message\n\n\
             2. 9:45 AM – 10:00 AM — Lunch\n   Personal\n   Summary: Ate."
        );
        assert_eq!(clipboard_text(&db, "2026-10-17", "2026-10-17").unwrap(), "Daytrace timeline · Today, Oct 17\n\nNo timeline activities were recorded for this day.");
        assert!(clipboard_text(&db, "yesterday", "2026-10-17").is_err());
    }
}
