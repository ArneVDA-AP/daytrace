use crate::db::Db;
use anyhow::{anyhow, Result};
use chrono::{Duration, NaiveDate};

/// Markdown export for an inclusive range of logical days. Returns (markdown, card count).
pub fn markdown(db: &Db, from_day: &str, to_day: &str) -> Result<(String, usize)> {
    let from = NaiveDate::parse_from_str(from_day, "%Y-%m-%d").map_err(|_| anyhow!("bad from date"))?;
    let to = NaiveDate::parse_from_str(to_day, "%Y-%m-%d").map_err(|_| anyhow!("bad to date"))?;
    let (from, to) = if from <= to { (from, to) } else { (to, from) };
    let mut out = String::new();
    let mut count = 0;
    let mut day = from;
    while day <= to {
        let key = day.format("%Y-%m-%d").to_string();
        let cards: Vec<_> = db.cards_for_day(&key)?.into_iter().filter(|c| c.category != crate::db::SYSTEM_CATEGORY).collect();
        if !cards.is_empty() {
            out += &format!("# {}\n\n", day.format("%A %-d %B %Y"));
            for c in &cards {
                out += &format!("## {} – {} · {}\n\n**{}**\n\n{}\n\n", c.start, c.end, c.category, c.title, c.summary.trim());
                if !c.distractions.is_empty() {
                    out += "Distractions:\n";
                    for d in &c.distractions {
                        out += &format!("- {} – {}: {}\n", d.start_time, d.end_time, d.title);
                    }
                    out += "\n";
                }
                count += 1;
            }
        }
        day += Duration::days(1);
    }
    if out.is_empty() {
        out = format!("No timeline entries between {from_day} and {to_day}.\n");
    }
    Ok((out, count))
}
