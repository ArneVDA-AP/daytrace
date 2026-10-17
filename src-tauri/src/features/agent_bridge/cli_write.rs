//! Command-line writes and PATH setup: `daytrace-mcp cli timeline update|delete`, `categories
//! add|rename|color|describe|remove`, `goal set`, `link`, `unlink`. Ported from Dayflow's
//! tools/dayflow-cli WriteCommands.swift and `dayflow link` / `unlink` (MIT, (c) 2025 Jerry Liu).
//!
//! Writes travel to the running app over the bridge and need "Allow edits". Reversible edits just
//! happen; destructive ones (activity delete, category remove) ask first in a terminal and need
//! `--yes` otherwise, because deleting an activity also removes its observations.
//!
//! Exit codes here: 4 edits off · 6 Daytrace not running (Dayflow uses 5, which Daytrace's reads
//! already use for "no data").

use super::bridge::BridgeError;
use super::cli::{Invocation, WriteCmd};
use super::queries::Source;
use serde_json::{json, Value};
use std::io::Write;

pub const EXIT_EDITS_OFF: i32 = 4;
pub const EXIT_APP_NOT_RUNNING: i32 = 6;

/// How a write reaches the app (the bridge; a fake in tests).
pub type Sender<'a> = &'a dyn Fn(&str, &Value) -> Result<Value, BridgeError>;
/// Asks a yes/no question; None when nobody can answer (not a terminal).
pub type Confirm<'a> = &'a mut dyn FnMut(&str) -> Option<bool>;

struct Failure {
    code: &'static str,
    message: String,
    exit: i32,
}

fn usage(message: impl Into<String>) -> Failure {
    Failure { code: "invalid_input", message: message.into(), exit: 2 }
}

/// The operation and arguments a write command sends.
fn request(inv: &Invocation, cmd: &WriteCmd) -> Result<(&'static str, Value, Option<String>), Failure> {
    match cmd {
        WriteCmd::Activity { sub, id } => {
            let id: i64 = id.as_deref().and_then(|s| s.trim().parse().ok()).ok_or_else(|| usage(format!("Usage: daytrace-mcp cli timeline {sub} <record-id>")))?;
            if sub == "delete" {
                let ask = format!("Delete activity {id}? Its observations are removed too; this can't be fully undone.");
                return Ok(("activity_delete", json!({"record_id": id}), Some(ask)));
            }
            let mut args = json!({"record_id": id});
            if let Some(t) = &inv.title {
                args["title"] = json!(t);
            }
            if let Some(c) = &inv.category {
                args["category"] = json!(c);
            }
            if args.as_object().map(|o| o.len()).unwrap_or(0) < 2 {
                return Err(usage("Nothing to change. Pass --title and/or --category."));
            }
            Ok(("activity_update", args, None))
        }
        WriteCmd::Category { sub, rest } => {
            let need = |n: usize, u: &str| if rest.len() >= n { Ok(()) } else { Err(usage(format!("Usage: daytrace-mcp cli categories {u}"))) };
            match sub.as_str() {
                "add" => {
                    need(1, "add <name> [--color <hex>]")?;
                    let mut args = json!({"name": rest.join(" ")});
                    if let Some(c) = &inv.color {
                        args["color"] = json!(c);
                    }
                    Ok(("category_add", args, None))
                }
                "rename" => {
                    need(2, "rename <old-name> <new-name>")?;
                    Ok(("category_update", json!({"name": rest[0], "new_name": rest[1..].join(" ")}), None))
                }
                "color" => {
                    need(2, "color <name> <hex>")?;
                    Ok(("category_update", json!({"name": rest[0], "color": rest[1]}), None))
                }
                "describe" => {
                    need(2, "describe <name> <text>")?;
                    Ok(("category_update", json!({"name": rest[0], "description": rest[1..].join(" ")}), None))
                }
                "remove" => {
                    need(1, "remove <name>")?;
                    let name = rest.join(" ");
                    let ask = format!("Delete the category \"{name}\"? Past activities keep the name and are shown in gray.");
                    Ok(("category_remove", json!({"name": name}), Some(ask)))
                }
                other => Err(usage(format!("Unknown subcommand \"{other}\". Use add, rename, color, describe, or remove."))),
            }
        }
        WriteCmd::Goal { date } => {
            let bad = || usage("Usage: daytrace-mcp cli goal set [YYYY-MM-DD] --focus-minutes <n> [--distraction-limit-minutes <n>]");
            let focus: i64 = inv.focus_minutes.as_deref().and_then(|s| s.trim().parse().ok()).ok_or_else(bad)?;
            let mut args = json!({"focus_minutes": focus});
            if let Some(d) = date {
                args["date"] = json!(super::queries::day_window(d).map_err(|e| usage(e.message()))?.day);
            }
            if let Some(l) = &inv.distraction_limit {
                args["distraction_limit_minutes"] = json!(l.trim().parse::<i64>().map_err(|_| bad())?);
            }
            Ok(("goal_set", args, None))
        }
    }
}

fn execute(inv: &Invocation, cmd: &WriteCmd, source: &Source, send: Sender, confirm: Confirm) -> Result<Option<Value>, Failure> {
    let (operation, args, ask) = request(inv, cmd)?;
    if !source.edits_enabled() {
        return Err(Failure { code: "edits_disabled", message: "Editing is off. Turn on \"Allow edits\" in Daytrace, under Settings > AI tools.".into(), exit: EXIT_EDITS_OFF });
    }
    if let Some(question) = ask {
        if !inv.yes {
            match confirm(&question) {
                None => return Err(usage("This is destructive. Run it again with --yes to confirm.")),
                Some(false) => return Ok(None),
                Some(true) => {}
            }
        }
    }
    send(operation, &args).map(Some).map_err(|e| Failure {
        exit: if e.code == "app_not_running" { EXIT_APP_NOT_RUNNING } else if e.code == "edits_disabled" { EXIT_EDITS_OFF } else { 1 },
        code: match e.code.as_str() {
            "app_not_running" => "app_not_running",
            "edits_disabled" => "edits_disabled",
            "not_found" => "not_found",
            "invalid_argument" | "already_exists" | "ambiguous" | "permission_denied" => "invalid_input",
            _ => "write_failed",
        },
        message: e.message,
    })
}

/// Runs a write command; returns the exit code.
pub fn run(inv: &Invocation, cmd: &WriteCmd, source: &Source, send: Sender, confirm: Confirm, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    match execute(inv, cmd, source, send, confirm) {
        Ok(None) => {
            let _ = writeln!(out, "Canceled.");
            0
        }
        Ok(Some(data)) => {
            let written = if inv.json {
                writeln!(out, "{}", serde_json::to_string_pretty(&super::queries::versioned(data)).unwrap_or_default())
            } else {
                writeln!(out, "✓ {}", data.get("message").and_then(Value::as_str).unwrap_or("Done."))
            };
            if written.is_err() { 1 } else { 0 }
        }
        Err(f) => {
            if inv.json {
                let _ = writeln!(err, "{}", json!({"schema_version": super::queries::SCHEMA_VERSION, "error": {"code": f.code, "message": f.message}}));
            } else {
                let _ = writeln!(err, "{}", f.message);
            }
            f.exit
        }
    }
}

/// `link` / `unlink`: the folder holding daytrace-mcp on or off the user's PATH.
pub fn run_link(add: bool, out: &mut dyn Write, err: &mut dyn Write) -> i32 {
    let Some(dir) = super::path::install_dir() else {
        let _ = writeln!(err, "Couldn't find the folder daytrace-mcp is in.");
        return 1;
    };
    let result = if add { super::path::link(&dir) } else { super::path::unlink(&dir) };
    match result {
        Ok(super::path::Change::Added) => {
            let _ = writeln!(out, "✓ Added {} to your PATH. Open a new terminal, then try: daytrace-mcp cli today", dir.display());
            0
        }
        Ok(super::path::Change::Removed) => {
            let _ = writeln!(out, "✓ Removed {} from your PATH.", dir.display());
            0
        }
        Ok(super::path::Change::Unchanged) => {
            let _ = writeln!(out, "{} {} on your PATH.", dir.display(), if add { "is already" } else { "was not" });
            0
        }
        Err(e) => {
            let _ = writeln!(err, "{e:#}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::cli::{parse, Command};
    use super::*;

    fn src(edits: bool) -> (tempfile::TempDir, Source) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("settings.json"), format!(r#"{{"allowAgentEdits":{edits}}}"#)).unwrap();
        let s = Source::from_dir(dir.path().to_path_buf());
        (dir, s)
    }

    fn go(line: &str, source: &Source, answer: Option<bool>) -> (i32, String, String, Vec<(String, Value)>) {
        let inv = parse(&line.split_whitespace().map(String::from).collect::<Vec<_>>()).unwrap();
        let Command::Write(cmd) = &inv.command else { panic!("not a write: {line}") };
        let sent = std::cell::RefCell::new(Vec::new());
        let send = |op: &str, args: &Value| -> Result<Value, BridgeError> {
            sent.borrow_mut().push((op.to_string(), args.clone()));
            match op {
                "activity_delete" if args["record_id"] == 9 => Err(BridgeError::new("not_found", "No activity with ID 9.")),
                _ => Ok(json!({"message": format!("did {op}")})),
            }
        };
        let mut confirm = |_q: &str| answer;
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&inv, cmd, source, &send, &mut confirm, &mut out, &mut err);
        (code, String::from_utf8(out).unwrap(), String::from_utf8(err).unwrap(), sent.into_inner())
    }

    #[test]
    fn write_commands_map_to_bridge_operations() {
        let (_d, s) = src(true);
        let (code, out, _, sent) = go("cli timeline update 12 --title Parser --category Work", &s, None);
        assert_eq!((code, out.as_str()), (0, "✓ did activity_update\n"));
        assert_eq!(sent[0].1, json!({"record_id": 12, "title": "Parser", "category": "Work"}));
        assert_eq!(go("cli timeline update 12", &s, None).0, 2);
        assert_eq!(go("cli timeline update x --title y", &s, None).0, 2);

        let (_, _, _, sent) = go("cli categories add Deep work --color #123456", &s, None);
        assert_eq!(sent[0], ("category_add".into(), json!({"name": "Deep work", "color": "#123456"})));
        let (_, _, _, sent) = go("cli categories rename Work Day job", &s, None);
        assert_eq!(sent[0].1, json!({"name": "Work", "new_name": "Day job"}));
        let (_, _, _, sent) = go("cli categories color Work #ABCDEF", &s, None);
        assert_eq!(sent[0].1, json!({"name": "Work", "color": "#ABCDEF"}));
        let (_, _, _, sent) = go("cli categories describe Work Paid work and school", &s, None);
        assert_eq!(sent[0].1, json!({"name": "Work", "description": "Paid work and school"}));
        assert_eq!(go("cli categories frob x", &s, None).0, 2);

        let (_, _, _, sent) = go("cli goal set 2026-03-11 --focus-minutes 240 --distraction-limit-minutes 30", &s, None);
        assert_eq!(sent[0], ("goal_set".into(), json!({"focus_minutes": 240, "date": "2026-03-11", "distraction_limit_minutes": 30})));
        assert_eq!(go("cli goal set", &s, None).0, 2);
        assert_eq!(go("cli goal set March --focus-minutes 5", &s, None).0, 2);
    }

    #[test]
    fn destructive_writes_ask_first() {
        let (_d, s) = src(true);
        // Not a terminal and no --yes: refused, nothing sent.
        let (code, _, err, sent) = go("cli timeline delete 12", &s, None);
        assert_eq!(code, 2);
        assert!(err.contains("--yes"));
        assert!(sent.is_empty());
        // Declined in the terminal.
        let (code, out, _, sent) = go("cli categories remove Music", &s, Some(false));
        assert_eq!((code, out.as_str()), (0, "Canceled.\n"));
        assert!(sent.is_empty());
        // Confirmed, or --yes.
        assert_eq!(go("cli categories remove Music", &s, Some(true)).3.len(), 1);
        let (code, _, _, sent) = go("cli timeline delete 12 --yes", &s, None);
        assert_eq!((code, sent[0].0.as_str()), (0, "activity_delete"));
        // The app's errors come through with their exit codes.
        let (code, _, err, _) = go("cli timeline delete 9 --yes --json", &s, None);
        assert_eq!(code, 1);
        assert_eq!(serde_json::from_str::<Value>(&err).unwrap()["error"]["code"], "not_found");
    }

    #[test]
    fn edits_off_and_app_closed() {
        let (_d, off) = src(false);
        let (code, _, err, sent) = go("cli categories add Reading", &off, None);
        assert_eq!(code, EXIT_EDITS_OFF);
        assert!(err.contains("Allow edits"));
        assert!(sent.is_empty());
        // Real bridge, no app listening.
        let (_d2, on) = src(true);
        let inv = parse(&["cli".into(), "categories".into(), "add".into(), "Reading".into(), "--json".into()]).unwrap();
        let Command::Write(cmd) = &inv.command else { unreachable!() };
        let send = |op: &str, a: &Value| super::super::bridge::send(&on.data_dir, op, a);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let code = run(&inv, cmd, &on, &send, &mut |_| None, &mut out, &mut err);
        assert_eq!(code, EXIT_APP_NOT_RUNNING);
        assert_eq!(serde_json::from_slice::<Value>(&err).unwrap()["error"]["code"], "app_not_running");
    }
}
