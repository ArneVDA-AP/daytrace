//! The command line: `daytrace-mcp cli <command> [arguments] [--json]`. Reads work with Daytrace
//! closed; they go straight to the database, read-only.
//!
//! Ported from Dayflow's tools/dayflow-cli (MIT, (c) 2025 Jerry Liu): main.swift (commands, flags,
//! help, exit codes) and Render.swift (text output, without terminal colors). The write commands
//! (`timeline update/delete`, `categories add/...`, `goal set`) and `link`/`unlink` are in
//! cli_write.rs.
//!
//! Exit codes: 0 success · 1 unexpected · 2 bad arguments · 3 not found · 4 edits off · 5 no
//! Daytrace data · 6 Daytrace not running (writes).

use super::mcp::weekly_json;
use super::queries::{self as q, Activity, QueryError, Source};
use crate::timeutil::local;
use serde_json::{json, Value};
use std::io::Write;

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Run the MCP server on stdio (no command, or `mcp`).
    Serve,
    Help,
    Version,
    Status,
    Timeline(Option<String>),
    Card(Option<String>),
    Daily(Option<String>),
    Weekly(Option<String>),
    Categories,
    Search(Option<String>),
    Days,
    Observations(Option<String>),
    /// Pause or resume recording: starts daytrace.exe with `--pause`/`--resume`
    /// (platform_extras::switches).
    Pause(Option<String>),
    Resume,
    /// A change sent to the running app.
    Write(WriteCmd),
    /// Add the daytrace-mcp folder to the user's PATH (`unlink`: remove it).
    Link,
    Unlink,
}

/// The write commands (cli_write.rs).
#[derive(Debug, Clone, PartialEq)]
pub enum WriteCmd {
    /// `timeline update|delete <id>`
    Activity { sub: String, id: Option<String> },
    /// `categories add|rename|color|describe|remove ...`
    Category { sub: String, rest: Vec<String> },
    /// `goal set [date]`
    Goal { date: Option<String> },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Detail {
    Compact,
    Summary,
    Detailed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Invocation {
    pub command: Command,
    pub json: bool,
    pub detail: Detail,
    pub category: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub force: bool,
    pub db: Option<String>,
    pub limit: Option<usize>,
    /// The MCP server for Daytrace's chat (read tools only).
    pub chat: bool,
    /// Confirms a destructive write without asking.
    pub yes: bool,
    pub title: Option<String>,
    pub color: Option<String>,
    pub focus_minutes: Option<String>,
    pub distraction_limit: Option<String>,
}

const VALUE_FLAGS: &[&str] = &["--category", "--from", "--to", "--db", "--limit", "--title", "--color", "--focus-minutes", "--distraction-limit-minutes"];
const BOOL_FLAGS: &[&str] = &["--json", "--summary", "--detailed", "--force", "--help", "--version", "--yes", "--chat"];

pub const HELP: &str = "daytrace-mcp — your Daytrace timeline for AI tools and the terminal

Usage
  daytrace-mcp [--db <path>]                 Run the MCP server on stdio (for Claude and other MCP clients)
  daytrace-mcp cli <command> [arguments] [--json]

Reading (works even when Daytrace is closed)
  status                     Recording state and today's date
  timeline [YYYY-MM-DD]      A day's activities (aliases: today, yesterday)
    --summary                Include one-line descriptions
    --detailed               Include full write-ups
    --category <name>        Only one category
    --from/--to <date>       A range; over 7 days returns totals (--force lists them)
  card <id>                  One activity in full detail
  observations <id>          The screen notes an activity was written from
  daily [YYYY-MM-DD]         The Daily standup
  weekly [YYYY-MM-DD]        The week containing that date
  categories                 Category names and colors
  search <text>              Find activities by title or summary
    --from/--to <date>       Only within these days
  days                       Days with activities, newest first
    --limit <n>              How many (default 30)

Recording (hands the switch to the running Daytrace, or starts it in the tray)
  pause <minutes|tomorrow|indefinite>   Pause recording (minutes from 1 to 1440)
  resume                     Resume recording

Editing (needs Daytrace running, with \"Allow edits\" on in Settings > AI tools)
  timeline update <id> --title <text> | --category <name>
  timeline delete <id>       Asks first; also removes its observations
  categories add <name> [--color <hex>]
  categories rename <old> <new>
  categories color <name> <hex>
  categories describe <name> <text>
  categories remove <name>   Asks first; past activities keep the name
  goal set [YYYY-MM-DD] --focus-minutes <n> [--distraction-limit-minutes <n>]
    --yes                    Confirm a delete without asking

Setup
  link                       Add the daytrace-mcp folder to your PATH
  unlink                     Remove it

Options
  --db <path>                The database (daytrace.sqlite) or its folder. Default:
                             %APPDATA%\\dev.daytrace.app, or DAYTRACE_DATA_DIR when set.
  --json                     Stable, scriptable output

Days run from 4 AM to 4 AM. Exit codes: 0 ok, 1 error, 2 bad arguments, 3 not found, 4 edits
off, 5 no data, 6 Daytrace not running.";

/// Parses the arguments after the program name.
pub fn parse(args: &[String]) -> Result<Invocation, String> {
    let mut inv = Invocation {
        command: Command::Serve,
        json: false,
        detail: Detail::Compact,
        category: None,
        from: None,
        to: None,
        force: false,
        db: None,
        limit: None,
        chat: false,
        yes: false,
        title: None,
        color: None,
        focus_minutes: None,
        distraction_limit: None,
    };
    let mut positional: Vec<String> = vec![];
    let mut flags: Vec<&str> = vec![];
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "-h" {
            flags.push("--help");
        } else if a.starts_with("--") {
            let (name, inline) = match a.split_once('=') {
                Some((n, v)) => (n, Some(v.to_string())),
                None => (a, None),
            };
            if VALUE_FLAGS.contains(&name) {
                let value = match inline {
                    Some(v) => v,
                    None => {
                        i += 1;
                        args.get(i).cloned().ok_or_else(|| format!("{name} needs a value."))?
                    }
                };
                match name {
                    "--category" => inv.category = Some(value),
                    "--from" => inv.from = Some(value),
                    "--to" => inv.to = Some(value),
                    "--db" => inv.db = Some(value),
                    "--title" => inv.title = Some(value),
                    "--color" => inv.color = Some(value),
                    "--focus-minutes" => inv.focus_minutes = Some(value),
                    "--distraction-limit-minutes" => inv.distraction_limit = Some(value),
                    _ => inv.limit = Some(value.trim().parse().map_err(|_| format!("--limit needs a number, not \"{value}\"."))?),
                }
            } else if let Some(f) = BOOL_FLAGS.iter().find(|f| **f == name) {
                flags.push(f);
            } else {
                return Err(format!("Unknown option {name}. Run `daytrace-mcp help`."));
            }
        } else {
            positional.push(a.to_string());
        }
        i += 1;
    }
    inv.json = flags.contains(&"--json");
    inv.force = flags.contains(&"--force");
    inv.yes = flags.contains(&"--yes");
    inv.chat = flags.contains(&"--chat");
    inv.detail = if flags.contains(&"--detailed") {
        Detail::Detailed
    } else if flags.contains(&"--summary") {
        Detail::Summary
    } else {
        Detail::Compact
    };

    // `cli` only marks command-line use; `daytrace-mcp today` works too.
    if positional.first().map(String::as_str) == Some("cli") {
        positional.remove(0);
        if positional.is_empty() && !flags.contains(&"--version") {
            inv.command = Command::Help;
            return Ok(inv);
        }
    }
    if flags.contains(&"--help") {
        inv.command = Command::Help;
        return Ok(inv);
    }
    if flags.contains(&"--version") {
        inv.command = Command::Version;
        return Ok(inv);
    }
    let arg = |n: usize| positional.get(n).cloned();
    inv.command = match positional.first().map(String::as_str) {
        None | Some("mcp") => Command::Serve,
        Some("help") => Command::Help,
        Some("version") => Command::Version,
        Some("status") => Command::Status,
        Some("timeline") if matches!(arg(1).as_deref(), Some("update" | "delete")) => {
            Command::Write(WriteCmd::Activity { sub: arg(1).unwrap_or_default(), id: arg(2) })
        }
        Some("timeline") => Command::Timeline(arg(1)),
        Some("today") => Command::Timeline(None),
        Some("yesterday") => Command::Timeline(Some("yesterday".into())),
        Some("card") => Command::Card(arg(1)),
        Some("observations") => Command::Observations(arg(1)),
        Some("daily") => Command::Daily(arg(1)),
        Some("weekly") => Command::Weekly(arg(1)),
        Some("categories") if positional.len() > 1 => Command::Write(WriteCmd::Category { sub: positional[1].clone(), rest: positional[2..].to_vec() }),
        Some("categories") => Command::Categories,
        Some("goal") if arg(1).as_deref() == Some("set") => Command::Write(WriteCmd::Goal { date: arg(2) }),
        Some("goal") => return Err("Usage: daytrace-mcp cli goal set [YYYY-MM-DD] --focus-minutes <n> ...".into()),
        Some("link") => Command::Link,
        Some("unlink") => Command::Unlink,
        Some("search") => Command::Search((positional.len() > 1).then(|| positional[1..].join(" "))),
        Some("days") => Command::Days,
        Some("pause") => Command::Pause(arg(1)),
        Some("resume") => Command::Resume,
        Some(other) => return Err(format!("Unknown command \"{other}\". Run `daytrace-mcp help`.")),
    };
    Ok(inv)
}

// ---- text helpers (Render.swift) ----

fn pad(s: &str, width: usize) -> String {
    let n = s.chars().count();
    if n >= width { s.to_string() } else { format!("{s}{}", " ".repeat(width - n)) }
}

/// "45m", "2h", "2h 05m" (Dayflow's formatDuration).
pub fn fmt_duration(minutes: i64) -> String {
    let (h, m) = (minutes / 60, minutes % 60);
    if h == 0 {
        format!("{m}m")
    } else if m == 0 {
        format!("{h}h")
    } else {
        format!("{h}h {m:02}m")
    }
}

/// "1 activity", "3 activities": a count with the noun in the right number.
fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn activities(n: usize) -> String {
    count(n, "activity", "activities")
}

fn clock(ts: i64) -> String {
    local(ts).format("%-I:%M %p").to_string()
}

fn day_header(ts: i64) -> String {
    local(ts).format("%A, %B %-d").to_string()
}

pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = vec![];
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph.split(' ').filter(|w| !w.is_empty()) {
            if current.is_empty() {
                current = word.to_string();
            } else if current.chars().count() + 1 + word.chars().count() <= width {
                current.push(' ');
                current.push_str(word);
            } else {
                lines.push(std::mem::take(&mut current));
                current = word.to_string();
            }
        }
        lines.push(current);
    }
    lines
}

fn bars(out: &mut Vec<String>, totals: &[(String, i64)], tracked: i64, dur_width: usize) {
    let name_width = totals.iter().map(|(n, _)| n.chars().count()).max().unwrap_or(10);
    let max = totals.first().map(|t| t.1).unwrap_or(1).max(1);
    for (name, minutes) in totals {
        let bar = "█".repeat((minutes * 20 / max).max(1) as usize);
        let pct = if tracked > 0 { minutes * 100 / tracked } else { 0 };
        out.push(format!("  {}  {}  {bar}  {pct}%", pad(name, name_width), pad(&fmt_duration(*minutes), dur_width)));
    }
}

fn render_timeline(acts: &[Activity], day: &q::DayWindow, detail: Detail) -> Vec<String> {
    if acts.is_empty() {
        return vec![format!("No activities recorded for {}.", day.day)];
    }
    let tracked: i64 = acts.iter().map(Activity::duration_minutes).sum();
    let mut out = vec![format!("{}  {} tracked", day_header(day.start), fmt_duration(tracked)), String::new()];
    let id_width = acts.iter().map(|a| a.record_id.to_string().len()).max().unwrap_or(4);
    for a in acts {
        let id = pad(&a.record_id.to_string(), id_width);
        let time = pad(&format!("{} – {}", clock(a.start), clock(a.end)), 20);
        match detail {
            Detail::Compact => out.push(format!("  {id}  {time}  {}  {}", pad(&a.title, 44), a.category)),
            Detail::Summary | Detail::Detailed => {
                out.push(format!("  {id}  {time}  {}  {}", a.title, a.category));
                let body = if detail == Detail::Detailed && !a.detailed_summary.is_empty() { &a.detailed_summary } else { &a.summary };
                for line in wrap(body, 68) {
                    out.push(format!("  {}  {line}", " ".repeat(id_width)));
                }
                out.push(String::new());
            }
        }
    }
    if detail == Detail::Compact {
        let mut totals: Vec<(String, i64)> = vec![];
        for a in acts {
            match totals.iter_mut().find(|(n, _)| *n == a.category) {
                Some(t) => t.1 += a.duration_minutes(),
                None => totals.push((a.category.clone(), a.duration_minutes())),
            }
        }
        totals.sort_by(|a, b| b.1.cmp(&a.1));
        out.push(String::new());
        out.push(format!("  {}", totals.iter().map(|(n, m)| format!("{n} {}", fmt_duration(*m))).collect::<Vec<_>>().join(" · ")));
        out.push(String::new());
        out.push(format!("  {} · daytrace-mcp cli timeline --summary for descriptions", activities(acts.len())));
    }
    out
}

fn list_line(a: &Activity, sep: &str) -> String {
    format!("  {}{sep}{} {}  {}", a.day, pad(&clock(a.start), 9), pad(&a.title, 44), a.category)
}

fn render_card(a: &Activity) -> Vec<String> {
    let mut out = vec![
        a.title.clone(),
        format!("{} · {} – {} · {}", day_header(a.start), clock(a.start), clock(a.end), fmt_duration(a.duration_minutes())),
    ];
    let mut context = a.category.clone();
    if !a.subcategory.is_empty() {
        context += &format!(" › {}", a.subcategory);
    }
    if !a.apps.is_empty() {
        context += &format!(" · {}", a.apps.join(", "));
    }
    out.push(context);
    out.push(String::new());
    if !a.summary.is_empty() {
        out.extend(wrap(&a.summary, 72));
        out.push(String::new());
    }
    if !a.detailed_summary.is_empty() {
        out.extend(wrap(&a.detailed_summary, 72));
    }
    out
}

// ---- running ----

struct Done {
    json: Option<Value>,
    text: Vec<String>,
}

fn both(json: Value, text: Vec<String>) -> Result<Done, QueryError> {
    Ok(Done { json: Some(json), text })
}

fn need_id(arg: &Option<String>, usage: &str) -> Result<i64, QueryError> {
    arg.as_deref().and_then(|s| s.trim().parse().ok()).ok_or_else(|| QueryError::Invalid(usage.into()))
}

fn day_or_today(arg: &Option<String>) -> Result<q::DayWindow, QueryError> {
    match arg {
        Some(d) => q::day_window(d),
        None => Ok(q::today_window()),
    }
}

fn execute(inv: &Invocation, source: &Source) -> Result<Done, QueryError> {
    use crate::features::platform_extras::switches;
    match &inv.command {
        Command::Pause(arg) => {
            let (json, text) = switches::cli_command(switches::cli_pause_choice(arg.as_deref()), &source.data_dir)?;
            both(json, text)
        }
        Command::Resume => {
            let (json, text) = switches::cli_command(Some(switches::Action::Resume), &source.data_dir)?;
            both(json, text)
        }
        Command::Serve | Command::Help | Command::Write(_) | Command::Link | Command::Unlink => Ok(Done { json: None, text: HELP.lines().map(String::from).collect() }),
        Command::Version => both(json!({"version": env!("CARGO_PKG_VERSION")}), vec![format!("daytrace-mcp {}", env!("CARGO_PKG_VERSION"))]),
        Command::Status => {
            let s = q::fetch_status(&source.open()?)?;
            let capture = match s.last_capture_at {
                None => "no captures found".to_string(),
                Some(t) => {
                    let minutes = (crate::timeutil::now_ts() - t) / 60;
                    if minutes < 2 { "last capture just now".into() } else { format!("last capture {} ago", fmt_duration(minutes)) }
                }
            };
            let mut s = s;
            s.edits_enabled = source.edits_enabled();
            s.app_running = Some(super::bridge::app_running(&source.data_dir));
            let mut text = vec![format!("  Daytrace · {capture}"), format!("  Today: {} (day starts 4:00 AM, {})", s.today, q::time_zone())];
            if s.pending_batches > 0 {
                text.push(format!("  Analysis backlog: {} batch{}", s.pending_batches, if s.pending_batches == 1 { "" } else { "es" }));
            }
            text.push(match (s.edits_enabled, s.app_running) {
                (false, _) => "  Edits: off (reads only)".to_string(),
                (true, Some(true)) => "  Edits: on".to_string(),
                (true, _) => "  Edits: on, but Daytrace isn't running".to_string(),
            });
            both(q::status_json(&s), text)
        }
        Command::Timeline(day) => {
            if inv.from.is_some() || inv.to.is_some() {
                return timeline_range(inv, source);
            }
            let w = day_or_today(day)?;
            let mut acts = q::fetch_day(&source.open()?, &w)?;
            if let Some(c) = &inv.category {
                acts.retain(|a| a.category.eq_ignore_ascii_case(c));
            }
            let detailed = inv.detail == Detail::Detailed;
            let hint = "Summaries are abbreviated. Use `daytrace-mcp cli card <record_id>` for the full write-up of a specific activity.";
            both(q::timeline_envelope(&acts, &w.day, detailed, hint), render_timeline(&acts, &w, inv.detail))
        }
        Command::Card(id) => {
            let id = need_id(id, "Usage: daytrace-mcp cli card <record-id>   (IDs come from `daytrace-mcp cli timeline`)")?;
            let a = q::fetch_activity(&source.open()?, id)?.ok_or_else(|| QueryError::NotFound(format!("No activity with ID {id}.")))?;
            both(q::activity_json(&a, true), render_card(&a))
        }
        Command::Observations(id) => {
            let id = need_id(id, "Usage: daytrace-mcp cli observations <record-id>")?;
            let conn = source.open()?;
            let a = q::fetch_activity(&conn, id)?.ok_or_else(|| QueryError::NotFound(format!("No activity with ID {id}.")))?;
            let rows = q::fetch_observations(&conn, &a)?;
            let mut text = vec![a.title.clone(), format!("{} · {} – {}", day_header(a.start), clock(a.start), clock(a.end)), String::new()];
            if rows.is_empty() {
                text.push("No observations for this activity: the user was away, it is being analyzed again, or an older Daytrace version deleted them (they are kept now).".into());
            }
            for o in &rows {
                let mut lines = wrap(&o.text, 64).into_iter();
                text.push(format!("  {}  {}", pad(&clock(o.start), 8), lines.next().unwrap_or_default()));
                for l in lines {
                    text.push(format!("  {}  {l}", " ".repeat(8)));
                }
            }
            both(q::observations_json(&a, &rows), text)
        }
        Command::Daily(day) => {
            let w = day_or_today(day)?;
            let s = q::fetch_standup(&source.open()?, &w.day)?
                .ok_or_else(|| QueryError::NotFound(format!("No Daily for {}. Daytrace generates one after a day with enough activity.", w.day)))?;
            let mut text = vec!["Highlights".to_string()];
            text.extend(s.highlights.iter().map(|h| format!("- {h}")));
            text.push(String::new());
            text.push("Tasks".into());
            text.extend(s.tasks.iter().map(|t| format!("- {t}")));
            text.push(String::new());
            text.push("Blockers".into());
            if s.blockers.is_empty() {
                text.push("- None".into());
            }
            text.extend(s.blockers.iter().map(|b| format!("- {b}")));
            both(q::standup_json(&s), text)
        }
        Command::Weekly(day) => {
            let w = day_or_today(day)?;
            let (start, end) = q::week_window(&w)?;
            let payload = weekly_json(source, &w)?;
            let totals = q::category_totals(&q::fetch_range(&source.open()?, start, end)?);
            let tracked = payload["tracked_minutes"].as_i64().unwrap_or(0);
            let focus = payload["focus_minutes"].as_i64().unwrap_or(0);
            let header = format!("{} – {}", local(start).format("%a %b %-d"), local(end - 86400).format("%a %b %-d"));
            let mut text = vec![format!("{header}  Tracked {} · Focus {}", fmt_duration(tracked), fmt_duration(focus)), String::new()];
            if tracked == 0 {
                text.push("  Nothing recorded this week.".into());
            } else {
                bars(&mut text, &totals, tracked, 8);
            }
            both(payload, text)
        }
        Command::Categories => {
            let cats = source.categories();
            let width = cats.iter().map(|c| c.name.chars().count()).max().unwrap_or(10);
            let text = cats.iter().map(|c| format!("  ■ {}  {}{}", pad(&c.name, width), c.color, if c.is_idle { "  system" } else { "" })).collect();
            both(q::categories_list_json(&cats), text)
        }
        Command::Search(text) => {
            let text = text.as_deref().map(str::trim).filter(|t| !t.is_empty()).ok_or_else(|| QueryError::Invalid("Usage: daytrace-mcp cli search <text>".into()))?;
            let range = match (&inv.from, &inv.to) {
                (None, None) => None,
                (f, t) => {
                    let today = q::today_window().day;
                    let (f, t) = q::range_windows(f.as_deref().unwrap_or("2000-01-01"), t.as_deref().unwrap_or(&today))?;
                    Some((f.start, t.end))
                }
            };
            let matches = q::search(&source.open()?, text, range, q::SEARCH_LIMIT)?;
            let mut lines = vec![];
            if matches.is_empty() {
                lines.push(format!("No activities matching \"{text}\"."));
            } else {
                lines.push(count(matches.len(), "match", "matches"));
                lines.push(String::new());
                lines.extend(matches.iter().map(|a| list_line(a, "  ")));
            }
            both(json!({"query": text, "matches": matches.iter().map(|a| q::activity_json(a, false)).collect::<Vec<_>>()}), lines)
        }
        Command::Days => {
            let rows = q::list_days(&source.open()?, inv.limit.unwrap_or(30).clamp(1, 3650))?;
            let text = if rows.is_empty() {
                vec!["No days with activities yet.".to_string()]
            } else {
                rows.iter()
                    .map(|d| format!("  {}  {}  {} tracked", d.day, pad(&activities(d.activities as usize), 15), fmt_duration(d.tracked_minutes)))
                    .collect()
            };
            both(q::days_json(&rows), text)
        }
    }
}

fn timeline_range(inv: &Invocation, source: &Source) -> Result<Done, QueryError> {
    let (Some(from), Some(to)) = (&inv.from, &inv.to) else {
        return Err(QueryError::Invalid("Ranges need --from and --to as YYYY-MM-DD, with --from first.".into()));
    };
    let (f, t) = q::range_windows(from, to).map_err(|_| QueryError::Invalid("Ranges need --from and --to as YYYY-MM-DD, with --from first.".into()))?;
    let days = q::days_between(&f, &t);
    let acts = q::fetch_range(&source.open()?, f.start, t.end)?;
    if days > q::MAX_RANGE_DAYS_FOR_ACTIVITIES && !inv.force {
        let totals = q::category_totals(&acts);
        let tracked: i64 = totals.iter().map(|t| t.1).sum();
        let mut text = vec![
            format!("{} → {}  {days} days · {} · {} tracked", f.day, t.day, activities(acts.len()), fmt_duration(tracked)),
            String::new(),
        ];
        bars(&mut text, &totals, tracked, 9);
        text.push(String::new());
        text.push(format!("  Showing totals — {days} days is too many to list."));
        text.push(format!("  Use a range of {} days or fewer, or --force to list all {}.", q::MAX_RANGE_DAYS_FOR_ACTIVITIES, acts.len()));
        return both(q::range_aggregate_json(&acts, &f.day, &t.day, days), text);
    }
    let detailed = inv.detail == Detail::Detailed;
    let mut text = vec![format!("{} · {} → {}", activities(acts.len()), f.day, t.day), String::new()];
    text.extend(acts.iter().map(|a| list_line(a, " ")));
    both(json!({"from": f.day, "to": t.day, "cards": acts.iter().map(|a| q::activity_json(a, detailed)).collect::<Vec<_>>()}), text)
}

/// Asks on the terminal; None when stdin is not one.
fn ask_terminal(question: &str) -> Option<bool> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        return None;
    }
    eprint!("{question} [y/N] ");
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer).ok()?;
    Some(matches!(answer.trim().to_lowercase().as_str(), "y" | "yes"))
}

/// Runs one CLI command; returns the exit code. Data goes to `out`, diagnostics to `err`.
pub fn run(inv: &Invocation, source: &Source, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match &inv.command {
        Command::Write(cmd) => {
            let send = |op: &str, args: &Value| super::bridge::send(&source.data_dir, op, args);
            return super::cli_write::run(inv, cmd, source, &send, &mut ask_terminal, out, err);
        }
        Command::Link => return super::cli_write::run_link(true, out, err),
        Command::Unlink => return super::cli_write::run_link(false, out, err),
        _ => {}
    }
    match execute(inv, source) {
        Ok(done) => {
            let written = match (inv.json, done.json) {
                (true, Some(v)) => writeln!(out, "{}", serde_json::to_string_pretty(&q::versioned(v)).unwrap_or_default()),
                (_, _) => writeln!(out, "{}", done.text.join("\n")),
            };
            if written.is_err() { 1 } else { 0 }
        }
        Err(e) => {
            if inv.json {
                let v = json!({"schema_version": q::SCHEMA_VERSION, "error": {"code": e.code(), "message": e.message()}});
                let _ = writeln!(err, "{v}");
            } else {
                let _ = writeln!(err, "{}", e.message());
            }
            e.exit_code()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::queries::fixture::*;
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    fn p(s: &str) -> Invocation {
        parse(&args(s)).unwrap()
    }

    #[test]
    fn parses_like_dayflow_cli() {
        assert_eq!(p("").command, Command::Serve);
        assert_eq!(p("mcp --db C:/x/daytrace.sqlite").db.as_deref(), Some("C:/x/daytrace.sqlite"));
        assert_eq!(p("cli").command, Command::Help);
        assert_eq!(p("--help").command, Command::Help);
        assert_eq!(p("cli today").command, Command::Timeline(None));
        assert_eq!(p("today").command, Command::Timeline(None));
        assert_eq!(p("cli yesterday --json").command, Command::Timeline(Some("yesterday".into())));
        let t = p("cli timeline 2026-03-11 --detailed --category Work");
        assert_eq!(t.command, Command::Timeline(Some("2026-03-11".into())));
        assert_eq!(t.detail, Detail::Detailed);
        assert_eq!(t.category.as_deref(), Some("Work"));
        // Flag values are not positional arguments.
        let r = p("cli timeline --from 2026-03-01 --to=2026-03-11 --force");
        assert_eq!(r.command, Command::Timeline(None));
        assert_eq!((r.from.as_deref(), r.to.as_deref(), r.force), (Some("2026-03-01"), Some("2026-03-11"), true));
        assert_eq!(p("cli search pricing page --from 2026-03-01").command, Command::Search(Some("pricing page".into())));
        assert_eq!(p("cli search").command, Command::Search(None));
        assert_eq!(p("cli card 12").command, Command::Card(Some("12".into())));
        assert_eq!(p("cli days --limit 5").limit, Some(5));
        assert_eq!(p("cli pause 30").command, Command::Pause(Some("30".into())));
        assert_eq!(p("cli resume --json").command, Command::Resume);
        assert_eq!(p("cli --version").command, Command::Version);
        assert!(parse(&args("cli frobnicate")).unwrap_err().contains("Unknown command"));
        assert!(parse(&args("cli today --bogus")).unwrap_err().contains("Unknown option --bogus"));
        assert!(parse(&args("cli timeline --from")).unwrap_err().contains("needs a value"));
        assert!(parse(&args("cli days --limit x")).is_err());
        // Writes and setup.
        let u = p("cli timeline update 12 --title Parser --category Work");
        assert_eq!(u.command, Command::Write(WriteCmd::Activity { sub: "update".into(), id: Some("12".into()) }));
        assert_eq!((u.title.as_deref(), u.category.as_deref()), (Some("Parser"), Some("Work")));
        assert_eq!(p("cli timeline delete 3 --yes").yes, true);
        assert_eq!(p("cli categories rename Work Day job").command, Command::Write(WriteCmd::Category { sub: "rename".into(), rest: vec!["Work".into(), "Day".into(), "job".into()] }));
        assert_eq!(p("cli categories").command, Command::Categories);
        let g = p("cli goal set 2026-03-11 --focus-minutes 240 --distraction-limit-minutes 30");
        assert_eq!(g.command, Command::Write(WriteCmd::Goal { date: Some("2026-03-11".into()) }));
        assert_eq!((g.focus_minutes.as_deref(), g.distraction_limit.as_deref()), (Some("240"), Some("30")));
        assert!(parse(&args("cli goal")).is_err());
        assert_eq!(p("cli link").command, Command::Link);
        assert_eq!(p("unlink").command, Command::Unlink);
        let chat = p("--chat");
        assert_eq!((chat.command, chat.chat), (Command::Serve, true));
    }

    fn run_str(f: &Fixture, line: &str) -> (i32, String, String) {
        let inv = p(line);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&inv, &f.source, &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap())
    }

    #[test]
    fn commands_against_a_temp_db() {
        let f = build();
        let (code, out, _) = run_str(&f, &format!("cli timeline {}", f.day));
        assert_eq!(code, 0);
        assert!(out.starts_with("Wednesday, March 11  2h 30m tracked"), "{out}");
        assert!(out.contains("Write the parser"));
        assert!(out.contains("Work 1h 45m · Personal 30m · System 15m"), "{out}");

        let (_, out, _) = run_str(&f, &format!("cli timeline {} --json --category work", f.day));
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["cards"].as_array().unwrap().len(), 2);
        assert_eq!(v["schema_version"], 1);

        let (_, out, _) = run_str(&f, &format!("cli timeline {} --summary", f.day));
        assert!(out.contains("Paid the electricity bill."));

        let (_, out, _) = run_str(&f, &format!("cli timeline --from 2026-03-01 --to {}", f.day));
        assert!(out.contains("Showing totals — 11 days is too many to list."), "{out}");
        let (_, out, _) = run_str(&f, &format!("cli timeline --from 2026-03-01 --to {} --force", f.day));
        assert!(out.starts_with("5 activities · 2026-03-01 → 2026-03-11"), "{out}");
        let (code, _, err) = run_str(&f, "cli timeline --from 2026-03-11");
        assert_eq!(code, 2);
        assert!(err.contains("Ranges need --from and --to"));

        let id = f.ids[2];
        let (_, out, _) = run_str(&f, &format!("cli card {id}"));
        assert!(out.contains("Work · github.com, VS Code"), "{out}");
        assert!(out.contains("Long write-up about the coverage report."));
        let (code, _, err) = run_str(&f, "cli card 424242 --json");
        assert_eq!(code, 3);
        assert_eq!(serde_json::from_str::<Value>(&err).unwrap()["error"]["code"], "not_found");
        assert_eq!(run_str(&f, "cli card abc").0, 2);

        let (_, out, _) = run_str(&f, &format!("cli observations {}", f.ids[0]));
        assert!(out.contains("Running cargo test."));

        assert_eq!(run_str(&f, "cli daily 2026-03-12").0, 3);
        add_recap(&f);
        let (code, out, _) = run_str(&f, "cli daily 2026-03-12");
        assert_eq!(code, 0);
        assert_eq!(out, "Highlights\n- Wrote the CSV parser\n\nTasks\n- Ship the parser\n\nBlockers\n- Waiting on review\n");

        let (_, out, _) = run_str(&f, &format!("cli weekly {}", f.day));
        assert!(out.starts_with("Mon Mar 9 – Sun Mar 15  Tracked 3h 15m · Focus 3h 15m"), "{out}");

        let (_, out, _) = run_str(&f, "cli search parser");
        assert!(out.starts_with("1 match\n"), "{out}");
        let (_, out, _) = run_str(&f, "cli search nothing-like-this");
        assert_eq!(out, "No activities matching \"nothing-like-this\".\n");

        let (_, out, _) = run_str(&f, "cli categories");
        assert!(out.contains("■ Idle         #A0AEC0  system"), "{out}");

        let (_, out, _) = run_str(&f, "cli days");
        assert!(!out.contains("1 activities"), "{out}");
        let (_, out, _) = run_str(&f, "cli days --json");
        assert_eq!(serde_json::from_str::<Value>(&out).unwrap()["days"].as_array().unwrap().len(), 2);

        let (_, out, _) = run_str(&f, "cli status");
        assert!(out.contains("Analysis backlog: 1 batch"), "{out}");

        // No database: exit code 5, JSON error on stderr.
        let empty = tempfile::tempdir().unwrap();
        let inv = p("cli today --json");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&inv, &Source::from_dir(empty.path().to_path_buf()), &mut out, &mut err);
        assert_eq!(code, 5);
        assert!(out.is_empty());
        assert_eq!(serde_json::from_str::<Value>(std::str::from_utf8(&err).unwrap()).unwrap()["error"]["code"], "no_data");
    }

    #[test]
    fn text_helpers() {
        assert_eq!(fmt_duration(45), "45m");
        assert_eq!(fmt_duration(120), "2h");
        assert_eq!(fmt_duration(125), "2h 05m");
        assert_eq!(wrap("one two three four", 9), ["one two", "three", "four"]);
        assert_eq!(pad("é", 3), "é  ");
        assert_eq!(activities(1), "1 activity");
        assert_eq!(activities(0), "0 activities");
        assert_eq!(count(2, "match", "matches"), "2 matches");
    }
}
