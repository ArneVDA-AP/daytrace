//! The changes AI tools may make, run by the app for requests that arrive over the bridge
//! (bridge.rs). Ported from Dayflow's Core/AgentAccess/AgentWriteHandlers.swift (MIT, (c) 2025
//! Jerry Liu): a fixed list of operations mapped onto the code paths the UI uses, never SQL.
//!
//! - Categories: the settings save path (config::save_lock, sanitize, save, then memory), with the
//!   rules of Settings > Categories: unique names, "System" reserved, Idle fixed.
//! - Activities: timeline_extras::cards update/delete, which mark edited cards so the next
//!   analysis window keeps them, and remove the observations under a deleted card.
//! - Day goals: journal::save_plan.
//!
//! Every request is refused unless "Allow edits" is on in the app's own settings, whatever the
//! client checked. Successful writes are appended to agent-writes.log in the data folder
//! (arguments only, as Dayflow does), so there is something to inspect afterwards. The log keeps
//! the retention period of the screenshots and at most `LOG_MAX_BYTES` (`trim_log`), since the
//! arguments hold activity titles.
//!
//! Category names and descriptions follow the Settings rules, and every description goes into
//! each analysis prompt: a name has at most `NAME_MAX` characters, a description at most
//! `DESCRIPTION_MAX` after its line breaks become spaces (as Settings does on save), and neither
//! may hold other control characters.
//!
//! Daytrace differences: Idle can't be renamed, recolored or removed (it is drawn as a hatch and
//! its prompt description is fixed), the last category besides Idle can't be removed, and day
//! goals have no per-day category lists (Daytrace counts focus and distraction by category, in
//! Settings > Categories), so `focus_categories` / `distraction_categories` are not used.

use super::bridge::BridgeError;
use crate::config::Settings;
use crate::db::SYSTEM_CATEGORY;
use crate::features::journal;
use crate::features::timeline_extras::cards;
use crate::state::AppState;
use crate::types::Category;
use serde_json::{json, Value};

pub const EDITS_OFF: &str = "Edits are turned off. The user can turn on \"Allow edits\" in Daytrace, under Settings > AI tools.";

/// The longest category name, in characters (Settings > Categories has the same limit).
pub const NAME_MAX: usize = 60;
/// The longest category description, in characters, after line breaks became spaces.
pub const DESCRIPTION_MAX: usize = 500;
/// agent-writes.log is cut back to half this size when it grows past it.
pub const LOG_MAX_BYTES: u64 = 1 << 20;
const LOG_FILE: &str = "agent-writes.log";

/// Colors offered for new categories (src/views/settings/category-colors.ts), in order.
const NEW_COLORS: [&str; 7] = ["#407BC5", "#287C42", "#73176F", "#BE6B0B", "#048F90", "#876CCA", "#8A8329"];

/// What a write changed, so the app can tell the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Changed {
    Settings,
    Timeline,
    Goals,
}

fn invalid(m: impl Into<String>) -> BridgeError {
    BridgeError::new("invalid_argument", m)
}

fn internal(e: impl std::fmt::Display) -> BridgeError {
    BridgeError::new("internal_error", format!("{e:#}"))
}

fn str_arg(args: &Value, key: &str) -> Option<String> {
    args.get(key).and_then(Value::as_str).map(|s| s.trim().to_string())
}

fn int_arg(args: &Value, key: &str) -> Option<i64> {
    match args.get(key)? {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().filter(|f| f.fract() == 0.0).map(|f| f as i64)),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
}

/// Runs one operation. Returns the reply data and what changed.
pub fn handle(state: &AppState, operation: &str, args: &Value) -> Result<(Value, Changed), BridgeError> {
    if !state.settings().allow_agent_edits {
        return Err(BridgeError::new("edits_disabled", EDITS_OFF));
    }
    let out = match operation {
        "category_add" => category_add(state, args).map(|v| (v, Changed::Settings)),
        "category_update" => category_update(state, args).map(|v| (v, Changed::Settings)),
        "category_remove" => category_remove(state, args).map(|v| (v, Changed::Settings)),
        "activity_update" => activity_update(state, args).map(|v| (v, Changed::Timeline)),
        "activity_delete" => activity_delete(state, args).map(|v| (v, Changed::Timeline)),
        "goal_set" => goal_set(state, args).map(|v| (v, Changed::Goals)),
        _ => return Err(BridgeError::new("unknown_operation", format!("Unknown operation: {operation}"))),
    }?;
    log_write(state, operation, args);
    Ok(out)
}

fn log_write(state: &AppState, operation: &str, args: &Value) {
    use std::io::Write;
    let at = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%:z").to_string();
    let line = json!({"at": at, "operation": operation, "arguments": args}).to_string();
    let path = log_path(state);
    {
        let _log = lock_log();
        let file = std::fs::OpenOptions::new().create(true).append(true).open(&path);
        if let Ok(mut f) = file {
            let _ = writeln!(f, "{line}");
        }
    }
    maintain_log(state);
}

/// Appends and trims one at a time, so a line is never written to a file being replaced.
fn lock_log() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn log_path(state: &AppState) -> std::path::PathBuf {
    state.paths.root.join(LOG_FILE)
}

/// Trims agent-writes.log to the retention period and the size cap (`trim_log`). Runs after each
/// write, at startup and hourly from the platform loop.
pub fn maintain_log(state: &AppState) {
    let cutoff = crate::timeutil::now_ts() - state.settings().retention_days as i64 * 86400;
    let _log = lock_log();
    if let Err(e) = trim_log(&log_path(state), cutoff, LOG_MAX_BYTES) {
        log::warn!("could not trim {LOG_FILE}: {e}");
    }
}

/// When a log line was written (its `at`, Unix seconds).
fn line_time(line: &str) -> Option<i64> {
    let v: Value = serde_json::from_str(line).ok()?;
    chrono::DateTime::parse_from_rfc3339(v.get("at")?.as_str()?).ok().map(|t| t.timestamp())
}

/// Drops the lines written before `cutoff` (Unix seconds) and lines that can't be read; when the
/// file is larger than `max_bytes`, keeps the newest lines that fit in half of it. The file is
/// rewritten only when something goes; it is read from its last `2 * max_bytes` at most.
pub fn trim_log(path: &std::path::Path, cutoff: i64, max_bytes: u64) -> std::io::Result<()> {
    use std::io::{BufRead, Read, Seek, SeekFrom};
    let Ok(mut file) = std::fs::File::open(path) else { return Ok(()) };
    let size = file.metadata()?.len();
    // The common case: small, and the first line is still within the retention period.
    if size <= max_bytes {
        let mut first = String::new();
        std::io::BufReader::new(&file).read_line(&mut first)?;
        if first.is_empty() || line_time(first.trim_end()).is_some_and(|t| t >= cutoff) {
            return Ok(());
        }
        file.seek(SeekFrom::Start(0))?;
    }
    let skip = size.saturating_sub(2 * max_bytes);
    file.seek(SeekFrom::Start(skip))?;
    let mut raw = Vec::new();
    (&mut file).take(2 * max_bytes).read_to_end(&mut raw)?;
    let text = String::from_utf8_lossy(&raw);
    let mut lines: Vec<&str> = text.lines().collect();
    if skip > 0 && !lines.is_empty() {
        lines.remove(0); // starts mid-line
    }
    let mut keep: Vec<&str> = lines.into_iter().filter(|l| line_time(l).is_some_and(|t| t >= cutoff)).collect();
    let total = |v: &[&str]| v.iter().map(|l| l.len() as u64 + 1).sum::<u64>();
    if total(&keep) > max_bytes {
        let mut sum = 0u64;
        let from = keep.iter().rposition(|l| {
            sum += l.len() as u64 + 1;
            sum > max_bytes / 2
        });
        keep.drain(..from.map(|i| i + 1).unwrap_or(0));
    }
    let mut out = keep.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    drop(file);
    let tmp = path.with_extension("log.tmp");
    std::fs::write(&tmp, out)?;
    std::fs::rename(&tmp, path)
}

// ---- categories ----

/// Changes the settings the way every other settings change does: under the save lock, the file
/// first, then memory.
fn change_settings(state: &AppState, f: impl FnOnce(&mut Settings) -> Result<Value, BridgeError>) -> Result<Value, BridgeError> {
    let _saving = crate::config::save_lock();
    let mut s = state.settings();
    let out = f(&mut s)?;
    let s = s.sanitized();
    s.save(&state.paths.settings).map_err(internal)?;
    state.replace_settings(s);
    Ok(out)
}

fn names(cats: &[Category]) -> String {
    cats.iter().map(|c| c.name.as_str()).collect::<Vec<_>>().join(", ")
}

/// The index of the category called `name` (case-insensitive).
fn resolve(cats: &[Category], name: &str) -> Result<usize, BridgeError> {
    let matches: Vec<usize> = cats.iter().enumerate().filter(|(_, c)| c.name.trim().eq_ignore_ascii_case(name.trim())).map(|(i, _)| i).collect();
    match matches.as_slice() {
        [] => Err(BridgeError::new("not_found", format!("No category named \"{}\". Categories: {}", name.trim(), names(cats)))),
        [i] => Ok(*i),
        _ => Err(BridgeError::new("ambiguous", format!("More than one category matches \"{}\".", name.trim()))),
    }
}

/// "#F96E00" from "F96E00" or "#f96e00".
pub fn valid_hex(value: &str) -> Result<String, BridgeError> {
    let v = value.trim();
    let hex = v.strip_prefix('#').unwrap_or(v);
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(invalid(format!("\"{v}\" is not a hex color like #BE6B0B.")));
    }
    Ok(format!("#{}", hex.to_ascii_uppercase()))
}

/// A description as Settings saves it: line breaks (and the spaces around them) become one space.
/// Refuses one that is too long or holds other control characters.
pub fn clean_description(raw: &str) -> Result<String, BridgeError> {
    static BREAKS: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    let re = BREAKS.get_or_init(|| regex::Regex::new(r"\s*[\r\n]+\s*").unwrap());
    let flat = re.replace_all(raw.trim(), " ").into_owned();
    if flat.chars().any(char::is_control) {
        return Err(invalid("A description can't contain control characters."));
    }
    if flat.chars().count() > DESCRIPTION_MAX {
        return Err(invalid(format!("A description can be at most {DESCRIPTION_MAX} characters.")));
    }
    Ok(flat)
}

fn check_new_name(cats: &[Category], name: &str, skip: Option<usize>) -> Result<(), BridgeError> {
    if name.is_empty() {
        return Err(invalid("A category name is required."));
    }
    if name.chars().any(char::is_control) {
        return Err(invalid("A category name can't contain control characters or line breaks."));
    }
    if name.chars().count() > NAME_MAX {
        return Err(invalid(format!("A category name can be at most {NAME_MAX} characters.")));
    }
    if name.eq_ignore_ascii_case(SYSTEM_CATEGORY) {
        return Err(invalid("System is used for cards that couldn't be analyzed. Pick another name."));
    }
    if cats.iter().enumerate().any(|(i, c)| Some(i) != skip && c.name.trim().eq_ignore_ascii_case(name)) {
        return Err(BridgeError::new("already_exists", format!("A category named \"{name}\" already exists.")));
    }
    Ok(())
}

fn category_add(state: &AppState, args: &Value) -> Result<Value, BridgeError> {
    let name = str_arg(args, "name").unwrap_or_default();
    let color = str_arg(args, "color").filter(|c| !c.is_empty()).map(|c| valid_hex(&c)).transpose()?;
    let description = clean_description(&str_arg(args, "description").unwrap_or_default())?;
    change_settings(state, |s| {
        check_new_name(&s.categories, &name, None)?;
        let color = color.unwrap_or_else(|| {
            NEW_COLORS.iter().find(|h| !s.categories.iter().any(|c| c.color.eq_ignore_ascii_case(h))).unwrap_or(&NEW_COLORS[3]).to_string()
        });
        // New categories go before Idle, as in Settings.
        let at = s.categories.iter().position(|c| c.is_idle).unwrap_or(s.categories.len());
        s.categories.insert(at, Category { name: name.clone(), color, description, is_idle: false, counts_as: None });
        Ok(json!({"message": format!("Created {name}")}))
    })
}

fn category_update(state: &AppState, args: &Value) -> Result<Value, BridgeError> {
    let name = str_arg(args, "name").filter(|n| !n.is_empty()).ok_or_else(|| invalid("A category name is required."))?;
    let new_name = str_arg(args, "new_name").filter(|n| !n.is_empty());
    let color = str_arg(args, "color").map(|c| valid_hex(&c)).transpose()?;
    let description = args.get("description").and_then(Value::as_str).map(clean_description).transpose()?;
    change_settings(state, |s| {
        let i = resolve(&s.categories, &name)?;
        let current = s.categories[i].name.clone();
        if s.categories[i].is_idle {
            return Err(BridgeError::new("permission_denied", format!("\"{current}\" is a system category for time away; it can't be changed.")));
        }
        let mut changes: Vec<String> = Vec::new();
        if let Some(n) = new_name.filter(|n| *n != current) {
            check_new_name(&s.categories, &n, Some(i))?;
            // Keep what the category counts as: a choice derived from the old name ("Distraction")
            // must not change with the new one.
            let counts = s.categories[i].counts();
            s.categories[i].counts_as = Some(counts);
            s.categories[i].name = n.clone();
            changes.push(format!("renamed to {n} (past activities keep the name {current})"));
        }
        if let Some(c) = color {
            s.categories[i].color = c.clone();
            changes.push(format!("color {c}"));
        }
        if let Some(d) = description {
            s.categories[i].description = d;
            changes.push("description updated".into());
        }
        if changes.is_empty() {
            return Err(invalid("Nothing to change. Pass new_name, color, or description."));
        }
        Ok(json!({"message": format!("{current}: {}", changes.join(", "))}))
    })
}

fn category_remove(state: &AppState, args: &Value) -> Result<Value, BridgeError> {
    let name = str_arg(args, "name").filter(|n| !n.is_empty()).ok_or_else(|| invalid("A category name is required."))?;
    change_settings(state, |s| {
        let i = resolve(&s.categories, &name)?;
        let c = s.categories[i].clone();
        if c.is_idle {
            return Err(BridgeError::new("permission_denied", format!("\"{}\" is a system category.", c.name)));
        }
        if s.categories.iter().filter(|k| !k.is_idle).count() <= 1 {
            return Err(invalid("Keep at least one category besides the one for time away."));
        }
        let affected: i64 = state
            .db
            .conn()
            .query_row("SELECT COUNT(*) FROM timeline_cards WHERE category = ? AND is_deleted = 0", [&c.name], |r| r.get(0))
            .map_err(internal)?;
        s.categories.remove(i);
        let message = if affected == 1 {
            format!("Removed {}. 1 past activity keeps the name and is shown in gray.", c.name)
        } else {
            format!("Removed {}. {affected} past activities keep the name and are shown in gray.", c.name)
        };
        Ok(json!({"message": message, "affected_activities": affected}))
    })
}

// ---- activities ----

fn record_id(args: &Value) -> Result<i64, BridgeError> {
    int_arg(args, "record_id").ok_or_else(|| invalid("record_id is required."))
}

/// Live (not deleted) card with this id.
fn live(state: &AppState, id: i64) -> Result<(), BridgeError> {
    let deleted: Option<i64> = state
        .db
        .conn()
        .query_row("SELECT is_deleted FROM timeline_cards WHERE id = ?", [id], |r| r.get(0))
        .ok();
    match deleted {
        Some(0) => Ok(()),
        _ => Err(BridgeError::new("not_found", format!("No activity with ID {id}."))),
    }
}

fn activity_update(state: &AppState, args: &Value) -> Result<Value, BridgeError> {
    let id = record_id(args)?;
    let title = str_arg(args, "title").filter(|t| !t.is_empty());
    let cats = state.settings().categories;
    // Every requested value is resolved before the write, so a typo can't partly apply.
    let category = match str_arg(args, "category") {
        Some(c) => Some(cats[resolve(&cats, &c)?].name.clone()),
        None => None,
    };
    if title.is_none() && category.is_none() {
        return Err(invalid("Nothing to change. Pass title and/or category."));
    }
    live(state, id)?;
    let mut changes = Vec::new();
    if title.is_some() {
        changes.push("title".to_string());
    }
    if let Some(c) = &category {
        changes.push(format!("category → {c}"));
    }
    cards::update_card(&state.db, id, &cards::CardEdit { title, category, subcategory: None }, &cats).map_err(|e| invalid(format!("{e:#}")))?;
    Ok(json!({"message": format!("Updated {id}: {}", changes.join(", "))}))
}

fn activity_delete(state: &AppState, args: &Value) -> Result<Value, BridgeError> {
    let id = record_id(args)?;
    if !cards::delete_card(&state.db, id).map_err(internal)? {
        return Err(BridgeError::new("not_found", format!("No activity with ID {id}.")));
    }
    Ok(json!({"message": format!("Deleted activity {id}")}))
}

// ---- day goals ----

fn minutes(args: &Value, key: &str) -> Result<Option<i64>, BridgeError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => match int_arg(args, key) {
            Some(m) if (0..=journal::MAX_MINUTES).contains(&m) => Ok(Some(m)),
            _ => Err(invalid(format!("{key} must be a whole number of minutes from 0 to {}.", journal::MAX_MINUTES))),
        },
    }
}

fn goal_set(state: &AppState, args: &Value) -> Result<Value, BridgeError> {
    let focus = minutes(args, "focus_minutes")?.filter(|m| *m > 0).ok_or_else(|| invalid("A positive focus_minutes value is required."))?;
    let day = match args.get("date") {
        None | Some(Value::Null) => crate::timeutil::logical_day(crate::timeutil::now_ts()),
        Some(Value::String(d)) if !d.trim().is_empty() => super::queries::day_window(d).map_err(|e| invalid(e.message()))?.day,
        Some(Value::String(_)) => return Err(invalid("date cannot be empty when provided.")),
        Some(_) => return Err(invalid("date must be a YYYY-MM-DD string when provided.")),
    };
    let existing = journal::effective_plan(&state.db, &day).map_err(internal)?;
    let limit = minutes(args, "distraction_limit_minutes")?.unwrap_or(existing.distraction_limit_minutes);
    journal::save_plan(&state.db, &day, focus, limit, false).map_err(internal)?;
    let mut message = format!("{day}: focus target {focus} min, distraction limit {limit} min");
    if args.get("focus_categories").is_some() || args.get("distraction_categories").is_some() {
        message.push_str(". Daytrace counts focus and distraction by category (Settings > Categories), so the category lists were not used");
    }
    Ok(json!({"message": message}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Paths;
    use crate::db::ResolvedCard;
    use crate::types::ActivityCard;

    fn state(edits: bool) -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let st = AppState::new(Paths::new(dir.path().to_path_buf()).unwrap()).unwrap();
        st.settings.write().unwrap().allow_agent_edits = edits;
        (dir, st)
    }

    fn card(st: &AppState, title: &str, cat: &str, s: i64) -> i64 {
        let c = ResolvedCard {
            start_ts: s,
            end_ts: s + 900,
            card: ActivityCard {
                start_time: crate::timeutil::fmt_clock(s),
                end_time: crate::timeutil::fmt_clock(s + 900),
                category: cat.into(),
                subcategory: String::new(),
                title: title.into(),
                summary: "s".into(),
                detailed_summary: String::new(),
                distractions: None,
                app_sites: None,
            },
            idle_meta: None,
        };
        st.db.insert_screenshot(s, "", 0, 0, None, None, None, None).unwrap();
        let shots = st.db.screenshots_in_range(s, s + 1).unwrap();
        let batch = st.db.create_batch(&shots).unwrap();
        st.db.replace_cards_in_range(s, s + 900, &[c], batch, false).unwrap();
        st.db.cards_in_range(s, s + 900, false).unwrap()[0].id
    }

    fn err(r: Result<(Value, Changed), BridgeError>) -> BridgeError {
        r.unwrap_err()
    }

    #[test]
    fn refused_while_edits_are_off() {
        let (_d, st) = state(false);
        let e = err(handle(&st, "category_add", &json!({"name": "Reading"})));
        assert_eq!(e.code, "edits_disabled");
        assert_eq!(e.message, EDITS_OFF);
        assert!(!st.paths.root.join("agent-writes.log").exists());
    }

    #[test]
    fn categories_follow_the_settings_rules() {
        let (_d, st) = state(true);
        let (v, changed) = handle(&st, "category_add", &json!({"name": " Reading ", "color": "be6b0b", "description": "Books"})).unwrap();
        assert_eq!(v["message"], "Created Reading");
        assert_eq!(changed, Changed::Settings);
        let cats = st.settings().categories;
        assert_eq!(cats.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Work", "Personal", "Distraction", "Reading", "Idle"]);
        assert_eq!(cats[3].color, "#BE6B0B");
        // Saved to disk too, through the normal save path.
        assert_eq!(Settings::load(&st.paths.settings).categories.len(), 5);
        // A new category without a color gets the first unused one.
        handle(&st, "category_add", &json!({"name": "Music"})).unwrap();
        assert_eq!(st.settings().categories[4].color, "#048F90");

        assert_eq!(err(handle(&st, "category_add", &json!({"name": "work"}))).code, "already_exists");
        assert_eq!(err(handle(&st, "category_add", &json!({"name": "system"}))).code, "invalid_argument");
        assert_eq!(err(handle(&st, "category_add", &json!({"name": "  "}))).code, "invalid_argument");
        assert_eq!(err(handle(&st, "category_add", &json!({"name": "X", "color": "#12345"}))).message, "\"#12345\" is not a hex color like #BE6B0B.");

        let (v, _) = handle(&st, "category_update", &json!({"name": "reading", "new_name": "Books", "color": "#876cca", "description": "Novels"})).unwrap();
        assert_eq!(v["message"], "Reading: renamed to Books (past activities keep the name Reading), color #876CCA, description updated");
        assert_eq!(err(handle(&st, "category_update", &json!({"name": "Books"}))).message, "Nothing to change. Pass new_name, color, or description.");
        assert_eq!(err(handle(&st, "category_update", &json!({"name": "Books", "new_name": "Work"}))).code, "already_exists");
        assert_eq!(err(handle(&st, "category_update", &json!({"name": "Idle", "color": "#000000"}))).code, "permission_denied");
        let nf = err(handle(&st, "category_update", &json!({"name": "Nope", "color": "#000000"})));
        assert_eq!(nf.code, "not_found");
        assert!(nf.message.contains("Categories: Work, Personal, Distraction, Books, Music, Idle"), "{}", nf.message);

        // Removing: cards keep their name; Idle and the last other category stay.
        let day0 = crate::timeutil::logical_day_bounds("2026-03-11").unwrap().0;
        card(&st, "Read a novel", "Books", day0 + 3600);
        let (v, _) = handle(&st, "category_remove", &json!({"name": "books"})).unwrap();
        assert_eq!(v["message"], "Removed Books. 1 past activity keeps the name and is shown in gray.");
        assert_eq!(v["affected_activities"], 1);
        assert_eq!(err(handle(&st, "category_remove", &json!({"name": "Idle"}))).code, "permission_denied");
        for n in ["Music", "Personal", "Distraction"] {
            handle(&st, "category_remove", &json!({"name": n})).unwrap();
        }
        assert_eq!(err(handle(&st, "category_remove", &json!({"name": "Work"}))).code, "invalid_argument");

        // Every successful write is in the log, arguments only.
        let log = std::fs::read_to_string(st.paths.root.join("agent-writes.log")).unwrap();
        assert_eq!(log.lines().count(), 7);
        let first: Value = serde_json::from_str(log.lines().next().unwrap()).unwrap();
        assert_eq!(first["operation"], "category_add");
        assert_eq!(first["arguments"]["name"], " Reading ");
    }

    #[test]
    fn category_text_is_capped_and_flattened() {
        let (_d, st) = state(true);
        let long = "x".repeat(NAME_MAX + 1);
        assert_eq!(err(handle(&st, "category_add", &json!({"name": long}))).message, "A category name can be at most 60 characters.");
        let fits = "é".repeat(NAME_MAX);
        handle(&st, "category_add", &json!({"name": fits})).unwrap();
        assert_eq!(err(handle(&st, "category_add", &json!({"name": "Two\nlines"}))).code, "invalid_argument");
        assert_eq!(err(handle(&st, "category_add", &json!({"name": "Bell\u{7}"}))).code, "invalid_argument");
        assert_eq!(err(handle(&st, "category_update", &json!({"name": "Work", "new_name": "W".repeat(61)}))).code, "invalid_argument");
        // Descriptions: line breaks become spaces, as in Settings; the result is capped.
        handle(&st, "category_add", &json!({"name": "Reading", "description": "Books,\r\n  articles\n\npapers "})).unwrap();
        let cat = |n: &str| st.settings().categories.into_iter().find(|c| c.name == n).unwrap();
        assert_eq!(cat("Reading").description, "Books, articles papers");
        handle(&st, "category_update", &json!({"name": "Reading", "description": "a\nb"})).unwrap();
        assert_eq!(cat("Reading").description, "a b");
        let e = err(handle(&st, "category_update", &json!({"name": "Reading", "description": "y".repeat(DESCRIPTION_MAX + 1)})));
        assert_eq!(e.message, "A description can be at most 500 characters.");
        assert_eq!(err(handle(&st, "category_update", &json!({"name": "Reading", "description": "tab\there"}))).code, "invalid_argument");
        assert_eq!(err(handle(&st, "category_add", &json!({"name": "Huge", "description": "z\n".repeat(400_000)}))).code, "invalid_argument");
        handle(&st, "category_update", &json!({"name": "Reading", "description": "y".repeat(DESCRIPTION_MAX)})).unwrap();
        assert_eq!(cat("Reading").description.len(), DESCRIPTION_MAX);
        // Nothing refused reached the settings.
        assert!(!st.settings().categories.iter().any(|c| c.name == "Huge" || c.description.contains('\t')));
    }

    fn log_line(at: &str, op: &str) -> String {
        json!({"at": at, "operation": op, "arguments": {"title": "Payroll for Jan"}}).to_string()
    }

    #[test]
    fn the_log_keeps_the_retention_period_and_a_size_cap() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("agent-writes.log");
        let t = |s: &str| chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp();
        trim_log(&path, 0, 1000).unwrap(); // no file: nothing to do
        assert!(!path.exists());
        let lines = [log_line("2026-03-01T10:00:00+01:00", "old"), log_line("2026-03-09T10:00:00+01:00", "kept"), "not json".to_string(), log_line("2026-03-10T10:00:00+01:00", "new")];
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();
        trim_log(&path, t("2026-03-05T00:00:00+01:00"), 1 << 20).unwrap();
        let ops: Vec<String> = std::fs::read_to_string(&path).unwrap().lines().map(|l| serde_json::from_str::<Value>(l).unwrap()["operation"].as_str().unwrap().to_string()).collect();
        assert_eq!(ops, ["kept", "new"]);
        // Within the period and small: left as it is.
        let before = std::fs::read_to_string(&path).unwrap();
        trim_log(&path, t("2026-03-05T00:00:00+01:00"), 1 << 20).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), before);
        // Past the cap: the newest lines that fit in half of it stay.
        let many: Vec<String> = (0..200).map(|i| log_line(&format!("2026-03-10T10:{:02}:{:02}+01:00", i / 60, i % 60), &format!("op{i}"))).collect();
        std::fs::write(&path, many.join("\n") + "\n").unwrap();
        let cap = 4000;
        trim_log(&path, 0, cap).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.len() as u64 <= cap / 2 && !text.is_empty(), "{}", text.len());
        assert!(text.ends_with(&format!("{}\n", many[199])));
        assert!(text.lines().all(|l| line_time(l).is_some()), "whole lines only");
        // A file far past the cap is read from its end only.
        std::fs::write(&path, many.join("\n").repeat(3)).unwrap();
        trim_log(&path, 0, cap).unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() <= cap / 2);
    }

    #[test]
    fn writes_trim_the_log() {
        let (_d, st) = state(true);
        std::fs::write(log_path(&st), log_line("2020-01-01T10:00:00+01:00", "category_add") + "\n").unwrap();
        handle(&st, "category_add", &json!({"name": "Reading"})).unwrap();
        let log = std::fs::read_to_string(log_path(&st)).unwrap();
        assert_eq!(log.lines().count(), 1, "the line from 2020 is past the retention period");
        assert!(log.contains("\"Reading\""));
    }

    #[test]
    fn activities_use_the_timeline_edit_paths() {
        let (_d, st) = state(true);
        let day0 = crate::timeutil::logical_day_bounds("2026-03-11").unwrap().0;
        let id = card(&st, "Write the parser", "Work", day0 + 3600);
        let (v, changed) = handle(&st, "activity_update", &json!({"record_id": id, "title": " Parser work ", "category": "personal"})).unwrap();
        assert_eq!(v["message"], format!("Updated {id}: title, category → Personal"));
        assert_eq!(changed, Changed::Timeline);
        let c = st.db.card(id).unwrap().unwrap();
        assert_eq!((c.title.as_str(), c.category.as_str()), ("Parser work", "Personal"));
        assert!(cards::is_user_edited(&st.db, id).unwrap(), "edited cards are protected from the next rewrite");

        // A typo in the category changes nothing, not even the title.
        let e = err(handle(&st, "activity_update", &json!({"record_id": id, "title": "Other", "category": "Wrok"})));
        assert_eq!(e.code, "not_found");
        assert_eq!(st.db.card(id).unwrap().unwrap().title, "Parser work");
        assert_eq!(err(handle(&st, "activity_update", &json!({"record_id": id}))).code, "invalid_argument");
        assert_eq!(err(handle(&st, "activity_update", &json!({"title": "x"}))).message, "record_id is required.");
        assert_eq!(err(handle(&st, "activity_update", &json!({"record_id": 999, "title": "x"}))).message, "No activity with ID 999.");

        let (v, _) = handle(&st, "activity_delete", &json!({"record_id": id.to_string()})).unwrap();
        assert_eq!(v["message"], format!("Deleted activity {id}"));
        assert_eq!(err(handle(&st, "activity_delete", &json!({"record_id": id}))).code, "not_found");
        assert_eq!(err(handle(&st, "activity_update", &json!({"record_id": id, "title": "x"}))).code, "not_found");
    }

    #[test]
    fn day_goals() {
        let (_d, st) = state(true);
        let (v, changed) = handle(&st, "goal_set", &json!({"date": "2026-03-11", "focus_minutes": 300})).unwrap();
        assert_eq!(v["message"], "2026-03-11: focus target 300 min, distraction limit 120 min");
        assert_eq!(changed, Changed::Goals);
        let p = journal::effective_plan(&st.db, "2026-03-11").unwrap();
        assert!(p.saved && !p.is_skipped);
        assert_eq!((p.focus_target_minutes, p.distraction_limit_minutes), (300, 120));
        // The limit carries over when not given; category lists are not used, and the reply says so.
        let (v, _) = handle(&st, "goal_set", &json!({"date": "2026-03-11", "focus_minutes": 200, "distraction_limit_minutes": 45, "focus_categories": ["Work"]})).unwrap();
        assert!(v["message"].as_str().unwrap().starts_with("2026-03-11: focus target 200 min, distraction limit 45 min. Daytrace counts"));
        let (v, _) = handle(&st, "goal_set", &json!({"date": "2026-03-11", "focus_minutes": 60})).unwrap();
        assert_eq!(v["message"], "2026-03-11: focus target 60 min, distraction limit 45 min");
        assert_eq!(err(handle(&st, "goal_set", &json!({"focus_minutes": 0}))).code, "invalid_argument");
        assert_eq!(err(handle(&st, "goal_set", &json!({"focus_minutes": 721}))).code, "invalid_argument");
        assert_eq!(err(handle(&st, "goal_set", &json!({"focus_minutes": 60, "date": "March 11"}))).code, "invalid_argument");
        assert_eq!(err(handle(&st, "goal_set", &json!({"focus_minutes": 60, "date": 5}))).code, "invalid_argument");
        // No date: today.
        handle(&st, "goal_set", &json!({"focus_minutes": 90})).unwrap();
        let today = crate::timeutil::logical_day(crate::timeutil::now_ts());
        assert_eq!(journal::effective_plan(&st.db, &today).unwrap().focus_target_minutes, 90);
        assert_eq!(err(handle(&st, "frobnicate", &json!({}))).code, "unknown_operation");
    }
}
