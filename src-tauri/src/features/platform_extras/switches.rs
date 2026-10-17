//! Pause and resume from outside the window: scripts, Task Scheduler, a desktop shortcut or a
//! Claude Code hook (Dayflow's AppDeepLinkRouter, `dayflow://start-recording` and
//! `dayflow://stop-recording`, for Shortcuts).
//!
//! - `daytrace.exe --pause <minutes>|tomorrow|indefinite` and `daytrace.exe --resume`. The first
//!   instance reads them at startup; a second launch hands its arguments to the running instance
//!   through the single-instance plugin, which applies them and exits.
//! - `daytrace://pause?minutes=N`. The NSIS installer registers the scheme under
//!   HKCU\Software\Classes from tauri.conf.json > plugins > deep-link > desktop > schemes, and
//!   Windows starts `daytrace.exe "<link>"`, so a link arrives as a launch argument like the
//!   switches (the tauri-plugin-deep-link crate is not needed for that on Windows). Any web page
//!   can open such a link after a browser prompt, so the link only pauses, for 1 to 1440
//!   minutes; resuming stays on the command line.
//! - `daytrace-mcp cli pause|resume` launches `daytrace.exe` with the switch (no other IPC).
//!
//! A launch with a switch or a link never shows the window, also when it was refused.

use super::pause::PauseChoice;
use crate::features::agent_bridge::queries::QueryError;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The URL scheme (tauri.conf.json > plugins > deep-link > desktop > schemes).
pub const SCHEME: &str = "daytrace";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Pause(PauseChoice),
    Resume,
}

/// What a launch asks for.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Launch {
    /// The last pause or resume given (a later switch wins).
    pub action: Option<Action>,
    /// `--minimized` (the autostart entry).
    pub minimized: bool,
    /// Switches that were not understood, for the log.
    pub rejected: Vec<String>,
}

impl Launch {
    /// The window stays hidden: started by Windows at sign-in, by a script or by a link.
    pub fn hidden(&self) -> bool {
        self.minimized || self.action.is_some() || !self.rejected.is_empty()
    }
}

/// Reads the arguments of a launch, without the program name. Unknown arguments are ignored
/// (Windows and the installer may add their own).
pub fn parse_args<S: AsRef<str>>(args: &[S]) -> Launch {
    let mut out = Launch::default();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_ref().trim();
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
            _ => (a, None),
        };
        match name.to_ascii_lowercase().as_str() {
            "--minimized" => out.minimized = true,
            "--resume" => out.action = Some(Action::Resume),
            "--pause" => {
                let value = inline.or_else(|| {
                    // The value is the next argument, unless that is another switch.
                    let next = args.get(i + 1).map(|v| v.as_ref().trim().to_string()).filter(|v| !v.starts_with("--"));
                    if next.is_some() {
                        i += 1;
                    }
                    next
                });
                match value.as_deref().and_then(|v| PauseChoice::parse(&v.to_ascii_lowercase())) {
                    Some(c) => out.action = Some(Action::Pause(c)),
                    None => out.rejected.push(format!("--pause {}", value.unwrap_or_default()).trim_end().to_string()),
                }
            }
            _ if is_link(a) => match parse_link(a) {
                Ok(action) => out.action = Some(action),
                Err(why) => out.rejected.push(format!("{a} ({why})")),
            },
            _ => {}
        }
        i += 1;
    }
    out
}

fn is_link(a: &str) -> bool {
    a.get(..SCHEME.len() + 1).is_some_and(|p| p.eq_ignore_ascii_case(&format!("{SCHEME}:")))
}

/// `daytrace://pause?minutes=N`, N from 1 to 1440. Everything else is refused.
pub fn parse_link(link: &str) -> Result<Action, &'static str> {
    let rest = link.get(SCHEME.len() + 1..).ok_or("not a Daytrace link")?;
    let rest = rest.trim_start_matches('/');
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    if !path.trim_end_matches('/').eq_ignore_ascii_case("pause") {
        return Err("links can only pause recording");
    }
    let mut minutes = None;
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        if k.eq_ignore_ascii_case("minutes") {
            minutes = Some(v.trim().parse::<u32>().map_err(|_| "minutes must be a whole number")?);
        }
    }
    match minutes {
        Some(m) if (1..=24 * 60).contains(&m) => Ok(Action::Pause(PauseChoice::Minutes(m))),
        Some(_) => Err("minutes must be from 1 to 1440"),
        None => Err("the link needs ?minutes=N"),
    }
}

/// The switches that ask a running Daytrace for `action`.
pub fn switch_args(action: Action) -> Vec<String> {
    match action {
        Action::Pause(c) => vec!["--pause".into(), c.id()],
        Action::Resume => vec!["--resume".into()],
    }
}

/// Applies a pause or resume asked for from outside the window.
pub fn apply(app: &tauri::AppHandle, action: Action, from: &str) {
    let result = match action {
        Action::Pause(c) => super::pause_recording(app, c),
        Action::Resume => super::resume_recording(app),
    };
    match result {
        Ok(()) => log::info!("{from}: {action:?} applied"),
        Err(e) => log::error!("{from}: {action:?} failed: {e:#}"),
    }
}

/// The single-instance callback: a second `daytrace.exe` was started with `args` (program name
/// first). A pause or resume is applied without showing the window; any other launch brings
/// the window up.
pub fn on_second_launch(app: &tauri::AppHandle, args: &[String]) {
    let launch = parse_args(args.get(1..).unwrap_or_default());
    for r in &launch.rejected {
        log::warn!("second launch: ignored {r}");
    }
    match launch.action {
        Some(action) => apply(app, action, "second launch"),
        // A refused link or switch does nothing (and opens nothing a web page could use), and
        // `--minimized` keeps the window closed.
        None if launch.hidden() => {}
        None => crate::tray::show_main(app),
    }
}

// ---- daytrace-mcp cli pause | resume ----

/// The Daytrace app next to this program (daytrace-mcp is installed in the same folder).
fn app_exe() -> Option<PathBuf> {
    let dir = std::env::current_exe().ok()?.parent()?.to_path_buf();
    let name = if cfg!(windows) { "daytrace.exe" } else { "daytrace" };
    let p = dir.join(name);
    p.is_file().then_some(p)
}

/// What settings.json says about recording.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordingState {
    pub enabled: bool,
    pub pause_until: Option<i64>,
    pub setup_done: bool,
}

pub fn read_state(data_dir: &Path) -> Option<RecordingState> {
    let raw = std::fs::read_to_string(data_dir.join("settings.json")).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    Some(RecordingState {
        enabled: v.get("recordingEnabled").and_then(Value::as_bool).unwrap_or(true),
        pause_until: v.get("pauseUntil").and_then(Value::as_i64),
        setup_done: v.get("onboardingCompleted").and_then(Value::as_bool).unwrap_or(true),
    })
}

/// Whether `state` shows `action` done (`asked_at`: when it was asked, Unix seconds).
pub fn reached(state: &RecordingState, action: Action, asked_at: i64) -> bool {
    match action {
        Action::Resume => state.enabled,
        Action::Pause(PauseChoice::Indefinite) => !state.enabled && state.pause_until.is_none(),
        Action::Pause(PauseChoice::Minutes(m)) => {
            let want = asked_at + m as i64 * 60;
            !state.enabled && state.pause_until.is_some_and(|t| (t - want).abs() <= 120)
        }
        Action::Pause(PauseChoice::Tomorrow) => !state.enabled && state.pause_until.is_some_and(|t| t > asked_at),
    }
}

fn describe(action: Action, state: Option<&RecordingState>) -> String {
    let mut s = match action {
        Action::Resume => "Recording resumed.".to_string(),
        Action::Pause(PauseChoice::Indefinite) => "Recording paused until you resume it.".to_string(),
        Action::Pause(c) => match state.and_then(|s| s.pause_until) {
            Some(t) => format!("Recording paused until {}.", crate::timeutil::fmt_clock(t)),
            None => format!("Recording paused for {}.", c.label().to_lowercase()),
        },
    };
    if state.is_some_and(|s| !s.setup_done) {
        s.push_str(" Daytrace waits for its first-run setup before it records.");
    }
    s
}

/// Starts `exe` with `args`, detached from this console program. On Windows it also leaves the
/// caller's job object when the job allows it, so a Daytrace started here keeps running after
/// the terminal or the agent session that ran `daytrace-mcp` closes.
fn spawn_detached(exe: &Path, args: &[String]) -> std::io::Result<()> {
    let command = || {
        let mut c = std::process::Command::new(exe);
        c.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        c
    };
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        let flags = DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP;
        // A job that does not allow breakaway refuses the flag (access denied): start it inside.
        match command().creation_flags(flags | CREATE_BREAKAWAY_FROM_JOB).spawn() {
            Ok(_) => Ok(()),
            Err(_) => command().creation_flags(flags).spawn().map(|_| ()),
        }
    }
    #[cfg(not(windows))]
    {
        command().spawn().map(|_| ())
    }
}

/// `daytrace-mcp cli pause <minutes|tomorrow|indefinite>` / `resume`: starts `daytrace.exe` with
/// the switch (the running app takes it over, or Daytrace starts in the tray), then waits up to
/// ten seconds for the settings file to show the change. Exit code 1 when it does not.
pub fn cli_command(action: Option<Action>, data_dir: &Path) -> Result<(Value, Vec<String>), QueryError> {
    let action = action.ok_or_else(|| QueryError::Invalid("Usage: daytrace-mcp cli pause <minutes|tomorrow|indefinite>  (minutes from 1 to 1440)".into()))?;
    let exe = app_exe().ok_or_else(|| QueryError::Db("Daytrace was not found next to daytrace-mcp, so recording can't be changed from here.".into()))?;
    let asked_at = crate::timeutil::now_ts();
    spawn_detached(&exe, &switch_args(action)).map_err(|e| QueryError::Db(format!("Couldn’t start Daytrace ({}): {e}", exe.display())))?;
    for _ in 0..40 {
        std::thread::sleep(Duration::from_millis(250));
        if let Some(s) = read_state(data_dir).filter(|s| reached(s, action, asked_at)) {
            let text = describe(action, Some(&s));
            return Ok((json!({"ok": true, "recording": s.enabled, "pauseUntil": s.pause_until, "message": text}), vec![text]));
        }
    }
    Err(QueryError::Db("Daytrace was asked to change recording, but its settings did not show the change within 10 seconds. Check the tray icon.".into()))
}

/// The `pause` argument of the command line (`30`, `tomorrow`, `indefinite`).
pub fn cli_pause_choice(arg: Option<&str>) -> Option<Action> {
    arg.and_then(|a| PauseChoice::parse(&a.trim().to_ascii_lowercase())).map(Action::Pause)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Launch {
        parse_args(args)
    }

    #[test]
    fn reads_switches() {
        assert_eq!(p(&[]), Launch::default());
        assert!(!p(&[]).hidden());
        assert_eq!(p(&["--minimized"]), Launch { minimized: true, ..Default::default() });
        assert_eq!(p(&["--pause", "30"]).action, Some(Action::Pause(PauseChoice::Minutes(30))));
        assert_eq!(p(&["--pause=tomorrow"]).action, Some(Action::Pause(PauseChoice::Tomorrow)));
        assert_eq!(p(&["--PAUSE", "Indefinite"]).action, Some(Action::Pause(PauseChoice::Indefinite)));
        assert_eq!(p(&["--resume"]).action, Some(Action::Resume));
        assert!(p(&["--resume"]).hidden(), "a scripted launch keeps the window closed");
        // The later switch wins; unknown arguments are ignored.
        assert_eq!(p(&["--pause", "15", "--whatever", "--resume"]).action, Some(Action::Resume));
        // A pause without a usable value does nothing.
        let l = p(&["--pause"]);
        assert_eq!((l.action, l.rejected), (None, vec!["--pause".to_string()]));
        let l = p(&["--pause", "--minimized"]);
        assert_eq!((l.action, l.minimized), (None, true), "the next switch is not taken as the value");
        assert_eq!(p(&["--pause", "0"]).action, None);
        assert_eq!(p(&["--pause", "1441"]).action, None);
        assert_eq!(p(&["--pause", "1440"]).action, Some(Action::Pause(PauseChoice::Minutes(1440))));
    }

    #[test]
    fn links_only_pause() {
        assert_eq!(parse_link("daytrace://pause?minutes=30"), Ok(Action::Pause(PauseChoice::Minutes(30))));
        assert_eq!(parse_link("DAYTRACE://Pause/?minutes=5&from=hook"), Ok(Action::Pause(PauseChoice::Minutes(5))));
        assert!(parse_link("daytrace://resume").is_err());
        assert!(parse_link("daytrace://pause").is_err(), "no open-ended pause from a web page");
        assert!(parse_link("daytrace://pause?minutes=indefinite").is_err());
        assert!(parse_link("daytrace://pause?minutes=0").is_err());
        assert!(parse_link("daytrace://pause?minutes=99999").is_err());
        assert!(parse_link("daytrace://settings?x=1").is_err());
        // As Windows passes it: the link is the only argument.
        let l = p(&["daytrace://pause?minutes=45"]);
        assert_eq!(l.action, Some(Action::Pause(PauseChoice::Minutes(45))));
        assert!(l.hidden());
        let l = p(&["daytrace://resume"]);
        assert_eq!(l.action, None);
        assert_eq!(l.rejected.len(), 1);
        assert!(l.hidden(), "a refused link opens nothing either");
        // Other schemes are not links of ours.
        assert_eq!(p(&["https://pause?minutes=5"]), Launch::default());
    }

    #[test]
    fn switch_round_trip() {
        for a in [Action::Resume, Action::Pause(PauseChoice::Minutes(90)), Action::Pause(PauseChoice::Tomorrow), Action::Pause(PauseChoice::Indefinite)] {
            assert_eq!(p(&switch_args(a).iter().map(String::as_str).collect::<Vec<_>>()).action, Some(a));
        }
        assert_eq!(cli_pause_choice(Some("45")), Some(Action::Pause(PauseChoice::Minutes(45))));
        assert_eq!(cli_pause_choice(Some("Tomorrow")), Some(Action::Pause(PauseChoice::Tomorrow)));
        assert_eq!(cli_pause_choice(Some("soon")), None);
        assert_eq!(cli_pause_choice(None), None);
    }

    #[test]
    fn confirms_from_the_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_state(dir.path()), None);
        std::fs::write(dir.path().join("settings.json"), r#"{"recordingEnabled": false, "pauseUntil": 1800001800, "onboardingCompleted": true}"#).unwrap();
        let s = read_state(dir.path()).unwrap();
        assert!(reached(&s, Action::Pause(PauseChoice::Minutes(30)), 1_800_000_000));
        assert!(!reached(&s, Action::Pause(PauseChoice::Minutes(30)), 1_700_000_000), "an older pause");
        assert!(reached(&s, Action::Pause(PauseChoice::Tomorrow), 1_800_000_000));
        assert!(!reached(&s, Action::Pause(PauseChoice::Indefinite), 1_800_000_000));
        assert!(!reached(&s, Action::Resume, 1_800_000_000));
        let on = RecordingState { enabled: true, pause_until: None, setup_done: false };
        assert!(reached(&on, Action::Resume, 0));
        assert!(describe(Action::Resume, Some(&on)).contains("first-run setup"));
        assert_eq!(describe(Action::Pause(PauseChoice::Indefinite), None), "Recording paused until you resume it.");
    }

    #[test]
    fn cli_refuses_a_missing_choice() {
        let e = cli_command(None, Path::new(".")).unwrap_err();
        assert_eq!(e.exit_code(), 2);
    }
}
