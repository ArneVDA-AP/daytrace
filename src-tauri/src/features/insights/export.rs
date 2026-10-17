//! Recap Markdown for reading on a phone: the recap made from a day, that day's numbers and its
//! cards. Written to `<dir>/Daytrace/<YYYY-MM-DD>.md` (the day the file describes).

use super::recap::RecapEntry;
use super::stats::DayStats;
use crate::db::StoredCard;
use crate::timeutil::fmt_clock;
use anyhow::Result;
use chrono::NaiveDate;
use std::path::{Path, PathBuf};

/// `2 h 15 min`, `45 min`, `0 min`.
pub fn duration(secs: i64) -> String {
    let mins = ((secs.max(0) as f64) / 60.0).round() as i64;
    let (h, m) = (mins / 60, mins % 60);
    match (h, m) {
        (0, m) => format!("{m} min"),
        (h, 0) => format!("{h} h"),
        (h, m) => format!("{h} h {m} min"),
    }
}

fn long_date(day: &str) -> String {
    NaiveDate::parse_from_str(day, "%Y-%m-%d").map(|d| d.format("%A %-d %B %Y").to_string()).unwrap_or_else(|_| day.to_string())
}

fn bullets(out: &mut String, items: &[String]) {
    if items.is_empty() {
        out.push_str("- None\n");
    }
    for i in items {
        out.push_str(&format!("- {}\n", i.trim()));
    }
}

/// The file for `day`. `recap` is the recap whose source day is `day`; `note` replaces the recap
/// section when there is none (for example the reason generation failed).
pub fn recap_markdown(day: &str, stats: &DayStats, recap: Option<&RecapEntry>, cards: &[StoredCard], note: Option<&str>) -> String {
    let mut out = format!("# {}\n\n## Recap\n\n", long_date(day));
    match recap {
        Some(r) => {
            out.push_str("### Highlights\n\n");
            bullets(&mut out, &r.highlights);
            out.push_str(&format!("\n### Tasks for {}\n\n", long_date(&r.target_day)));
            bullets(&mut out, &r.tasks);
            out.push_str("\n### Blockers\n\n");
            bullets(&mut out, &r.blockers);
            out.push('\n');
        }
        None => out.push_str(&format!("{}\n\n", note.unwrap_or("No recap has been generated for this day."))),
    }

    out.push_str("## Day in numbers\n\n");
    if stats.captured_secs == 0 {
        out.push_str("Nothing was recorded on this day.\n\n");
    } else {
        let share = |secs: i64| if stats.active_secs > 0 { (secs as f64 / stats.active_secs as f64 * 100.0).round() as i64 } else { 0 };
        out.push_str(&format!("- Recorded: {}\n", duration(stats.captured_secs)));
        out.push_str(&format!("- Focused: {} ({}% of active time)\n", duration(stats.focus_secs), share(stats.focus_secs)));
        out.push_str(&format!("- Distracted: {} ({}% of active time)\n", duration(stats.distracted_secs), share(stats.distracted_secs)));
        out.push_str(&format!("- Idle: {}\n", duration(stats.idle_secs)));
        if let Some(b) = &stats.longest_focus {
            out.push_str(&format!("- Longest focus block: {} – {} ({})\n", fmt_clock(b.start_ts), fmt_clock(b.end_ts), duration(b.secs)));
        }
        out.push_str(&format!("- Context switches: {}\n", stats.context_switches));
        out.push_str(&format!("- Cards with interruptions: {}\n\n", stats.interruptions));
        out.push_str("### Time per category\n\n");
        for t in &stats.totals {
            out.push_str(&format!("- {}: {}\n", t.name, duration(t.secs)));
        }
        out.push('\n');
        if !stats.programs.is_empty() {
            out.push_str("### Programs\n\n");
            for p in stats.programs.iter().take(5) {
                out.push_str(&format!("- {}: {}\n", p.name, duration(p.secs)));
            }
            out.push('\n');
        }
    }

    out.push_str("## Distractions\n\n");
    if stats.distractions.is_empty() {
        out.push_str("No distractions were noted.\n\n");
    } else {
        for d in &stats.distractions {
            let within = d.within_card.as_ref().map(|c| format!(", during “{c}”")).unwrap_or_default();
            out.push_str(&format!("- {} – {}: {} ({}{})\n", fmt_clock(d.start_ts), fmt_clock(d.end_ts), d.title, duration(d.secs), within));
        }
        out.push('\n');
    }

    out.push_str("## Timeline\n\n");
    let cards: Vec<&StoredCard> = cards.iter().filter(|c| !c.category.eq_ignore_ascii_case(crate::db::SYSTEM_CATEGORY)).collect();
    if cards.is_empty() {
        out.push_str("No cards for this day.\n");
    }
    for c in cards {
        out.push_str(&format!("### {} – {} · {}\n\n**{}**\n\n", fmt_clock(c.start_ts), fmt_clock(c.end_ts), c.category, c.title.trim()));
        if !c.summary.trim().is_empty() {
            out.push_str(&format!("{}\n\n", c.summary.trim()));
        }
    }
    out
}

pub const RELATIVE_DIR: &str = "The recap folder must be a full path, such as C:\\Users\\you\\OneDrive. Choose it with Choose folder.";

/// The export folder must be absolute: a relative one would land wherever the app happens to
/// run from (with autostart on Windows, often C:\Windows\System32).
pub fn check_dir(dir: &str) -> Result<()> {
    let d = dir.trim();
    if !d.is_empty() && !Path::new(d).is_absolute() {
        anyhow::bail!(RELATIVE_DIR);
    }
    Ok(())
}

/// `<dir>/Daytrace/<day>.md`
pub fn export_path(dir: &str, day: &str) -> PathBuf {
    Path::new(dir.trim()).join("Daytrace").join(format!("{day}.md"))
}

/// Writes through a temporary file so a phone sync never picks up half a file.
pub fn write_file(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::insights::stats::compute_day;
    use crate::features::insights::stats::tests::{at, card, distraction};
    use crate::timeutil::logical_day_bounds;
    use crate::types::default_categories;

    fn sample() -> (DayStats, Vec<StoredCard>) {
        let d = (2026, 10, 16);
        let mut w = card(1, at(d, 9, 0), at(d, 11, 15), "Work", "Pricing page");
        w.distractions = vec![distraction(at(d, 9, 40), at(d, 9, 45), "Checked X")];
        let cards = vec![w, card(2, at(d, 11, 15), at(d, 11, 30), "System", "Processing failed")];
        let (s, e) = logical_day_bounds("2026-10-16").unwrap();
        (compute_day("2026-10-16", &cards, &default_categories(), s, e), cards)
    }

    #[test]
    fn markdown_with_recap() {
        let (st, cards) = sample();
        let r = RecapEntry {
            target_day: "2026-10-17".into(),
            source_day: "2026-10-16".into(),
            highlights: vec!["Finished the pricing page FAQ.".into()],
            tasks: vec!["Send it to Lisa.".into()],
            blockers: vec![],
            provider: "anthropic".into(),
            model: "m".into(),
            edited: false,
            created_at: 0,
            updated_at: 0,
        };
        let md = recap_markdown("2026-10-16", &st, Some(&r), &cards, None);
        assert!(md.starts_with("# Friday 16 October 2026\n\n## Recap\n\n### Highlights\n\n- Finished the pricing page FAQ.\n\n### Tasks for Saturday 17 October 2026\n\n- Send it to Lisa.\n\n### Blockers\n\n- None\n\n## Day in numbers"), "{md}");
        // The 5 minutes noted inside the Work card stay focus time, so the longest block (2 h 15 min)
        // equals the focus total instead of exceeding it.
        assert!(md.contains("- Recorded: 2 h 15 min\n- Focused: 2 h 15 min (100% of active time)\n- Distracted: 0 min (0% of active time)\n"), "{md}");
        assert!(md.contains("- Longest focus block: 9:00 AM – 11:15 AM (2 h 15 min)\n"));
        assert!(md.contains("- 9:40 AM – 9:45 AM: Checked X (5 min, during “Pricing page”)\n"));
        assert!(md.contains("### 9:00 AM – 11:15 AM · Work\n\n**Pricing page**\n\nPricing page summary\n\n"));
        assert!(!md.contains("Processing failed"));
    }

    #[test]
    fn markdown_without_recap_or_cards() {
        let (s, e) = logical_day_bounds("2026-10-16").unwrap();
        let st = compute_day("2026-10-16", &[], &default_categories(), s, e);
        let md = recap_markdown("2026-10-16", &st, None, &[], Some("The recap could not be generated: no key."));
        assert!(md.contains("## Recap\n\nThe recap could not be generated: no key.\n\n## Day in numbers\n\nNothing was recorded on this day."));
        assert!(md.ends_with("## Timeline\n\nNo cards for this day.\n"));
    }

    #[test]
    fn writes_into_daytrace_folder() {
        let dir = tempfile::tempdir().unwrap();
        let path = export_path(&dir.path().to_string_lossy(), "2026-10-16");
        assert!(path.ends_with("Daytrace/2026-10-16.md"));
        write_file(&path, "# one").unwrap();
        write_file(&path, "# two").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# two");
        let names: Vec<_> = std::fs::read_dir(path.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(names.len(), 1, "no temporary file left behind");
    }

    #[test]
    fn export_folder_must_be_absolute() {
        let dir = tempfile::tempdir().unwrap();
        assert!(check_dir(&dir.path().to_string_lossy()).is_ok());
        assert!(check_dir("").is_ok(), "empty turns the export off");
        assert_eq!(check_dir("OneDrive/Daytrace").unwrap_err().to_string(), RELATIVE_DIR);
        assert!(check_dir("./phone").is_err());
    }

    #[test]
    fn durations() {
        assert_eq!(duration(0), "0 min");
        assert_eq!(duration(45 * 60 + 20), "45 min");
        assert_eq!(duration(3600), "1 h");
        assert_eq!(duration(8100), "2 h 15 min");
    }
}
