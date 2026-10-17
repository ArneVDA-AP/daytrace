//! Installing updates (Dayflow: Sparkle with a silent user driver, System/UpdaterManager.swift
//! and SilentUserDriver.swift). The daily check (`update.rs`) finds a newer GitHub release. With
//! "Install updates automatically" on, tauri-plugin-updater reads the release's signed
//! `latest.json`, downloads the installer and checks its signature against the public key in
//! tauri.conf.json. Daytrace runs the installer (NSIS, passive: a progress bar, no questions) once
//! the PC has been idle for 10 minutes without a video playing, or is locked, and no batch is
//! being analyzed. The installer starts Daytrace again with `--minimized`, so it comes back in the
//! tray. A release without a signed `latest.json` (built without the signing key) keeps the
//! notification and the release page.
//!
//! The pure rules live here; `platform_extras.rs` holds the state and the plugin calls.

use serde::Serialize;
use std::path::Path;

/// Minutes without keyboard or mouse input before an update may install.
pub const IDLE_BEFORE_INSTALL: u64 = 10 * 60;
/// After a failed download the next try waits this long (or a newer release).
pub const RETRY_AFTER_FAILURE: i64 = 6 * 3600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Stage {
    /// Nothing downloaded.
    #[default]
    Idle,
    Downloading,
    /// Downloaded and verified; waits for the PC to be idle or locked.
    Ready,
    /// The installer was started; Daytrace is about to quit.
    Installing,
    Failed,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallStatus {
    pub stage: Stage,
    /// The version being downloaded, ready or failed.
    pub version: Option<String>,
    /// Bytes downloaded so far and the size, when the server says.
    pub downloaded: u64,
    pub total: Option<u64>,
    pub error: Option<String>,
    /// When the last download failed (Unix seconds).
    pub failed_at: Option<i64>,
}

impl InstallStatus {
    pub const fn idle() -> InstallStatus {
        InstallStatus { stage: Stage::Idle, version: None, downloaded: 0, total: None, error: None, failed_at: None }
    }
}

/// What the PC is doing when an install is considered.
#[derive(Debug, Clone, Copy, Default)]
pub struct Moment {
    pub idle_secs: u64,
    pub locked: bool,
    /// An app keeps the display on (a video, a call): someone is watching without typing.
    pub display_required: bool,
    /// A batch is being analyzed; quitting now would throw away a model request.
    pub analyzing: bool,
}

/// Whether a downloaded update may install now: locked, or idle for 10 minutes with nothing
/// keeping the display on; never in the middle of an analysis.
pub fn may_install(m: &Moment) -> bool {
    if m.analyzing {
        return false;
    }
    m.locked || (m.idle_secs >= IDLE_BEFORE_INSTALL && !m.display_required)
}

/// Whether two version strings name the same version ("v0.3.0" and "0.3.0" do).
pub fn same_version(a: &str, b: &str) -> bool {
    match (super::update::Version::parse(a), super::update::Version::parse(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a.trim() == b.trim(),
    }
}

/// Whether to start downloading `latest` now.
pub fn should_prepare(status: &InstallStatus, latest: &str, now: i64) -> bool {
    let same = status.version.as_deref().is_some_and(|v| same_version(v, latest));
    match status.stage {
        Stage::Idle => true,
        Stage::Downloading | Stage::Installing => false,
        // A newer release appeared after the download.
        Stage::Ready => !same,
        Stage::Failed => !same || status.failed_at.is_none_or(|t| now - t >= RETRY_AFTER_FAILURE || now < t),
    }
}

/// Whether Daytrace installs `version` by itself, so the "new version" notification is left out.
pub fn installs_itself(status: &InstallStatus, version: &str) -> bool {
    matches!(status.stage, Stage::Downloading | Stage::Ready | Stage::Installing) && status.version.as_deref().is_some_and(|v| same_version(v, version))
}

/// Arguments for the NSIS installer after the passive switch the plugin adds: `/R` starts
/// Daytrace when the installer is done, with the arguments after `/ARGS`. Automatic installs
/// bring it back in the tray; "Install and restart now" brings the window back.
pub fn installer_args(minimized: bool) -> Vec<String> {
    let mut v = vec!["/R".to_string(), "/ARGS".to_string()];
    if minimized {
        v.push("--minimized".into());
    }
    v
}

/// The MCP server next to the app. An MCP client (Claude Code, Claude Desktop) can keep it
/// running all day, and Windows does not let the installer replace a running program, but it
/// does let it be renamed.
pub const MCP_EXE: &str = "daytrace-mcp.exe";

/// The name a copy moved aside at `stamp` (Unix seconds) gets, `n` counting names already taken:
/// `daytrace-mcp.exe.1800000000.old`, `daytrace-mcp.exe.1800000000-1.old`. Each update has a
/// name of its own, because a copy moved aside by an earlier update may still be running (and so
/// can be neither replaced nor removed). The uninstaller removes `daytrace-mcp*.old`.
fn aside_name(stamp: i64, n: u32) -> String {
    if n == 0 { format!("{MCP_EXE}.{stamp}.old") } else { format!("{MCP_EXE}.{stamp}-{n}.old") }
}

/// When a copy was moved aside, read from its name; 0 for 0.3.0's `daytrace-mcp.exe.old`. None
/// for files that are not such copies.
fn aside_stamp(name: &str) -> Option<(i64, u32)> {
    let lower = name.to_ascii_lowercase();
    let middle = lower.strip_prefix(&format!("{MCP_EXE}."))?.strip_suffix("old")?;
    if middle.is_empty() {
        return Some((0, 0));
    }
    let middle = middle.strip_suffix('.')?;
    let (stamp, n) = middle.split_once('-').unwrap_or((middle, "0"));
    Some((stamp.parse().ok()?, n.parse().ok()?))
}

/// The copies moved aside in `dir`, oldest first.
fn asides(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut found: Vec<((i64, u32), std::path::PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some((aside_stamp(&e.file_name().to_string_lossy())?, e.path())))
        .filter(|(_, p)| p.is_file())
        .collect();
    found.sort();
    found.into_iter().map(|(_, p)| p).collect()
}

/// Before the installer runs: moves `daytrace-mcp.exe` aside so the installer can write the new
/// one. Running copies keep working from the renamed file. Returns the copy's new path, or None
/// when there was nothing to move.
pub fn move_mcp_aside(dir: &Path, now: i64) -> std::io::Result<Option<std::path::PathBuf>> {
    let exe = dir.join(MCP_EXE);
    if !exe.is_file() {
        return Ok(None);
    }
    let aside = (0..100).map(|n| dir.join(aside_name(now, n))).find(|p| !p.exists()).ok_or_else(|| std::io::Error::other("no free name to move it aside"))?;
    std::fs::rename(&exe, &aside)?;
    Ok(Some(aside))
}

/// At startup, and when the installer could not be started: puts the newest copy back when no
/// `daytrace-mcp.exe` is in place (the update did not install one), and removes the other copies.
/// A copy that is still running can't be removed and stays until a later start.
pub fn tidy_mcp_after_update(dir: &Path) {
    let mut left = asides(dir);
    let exe = dir.join(MCP_EXE);
    if !exe.is_file()
        && let Some(newest) = left.pop()
    {
        match std::fs::rename(&newest, &exe) {
            Ok(()) => log::info!("{MCP_EXE} was not installed by the update; put the previous one back"),
            Err(e) => {
                log::warn!("could not put {MCP_EXE} back: {e}");
                left.push(newest);
            }
        }
    }
    for old in left {
        match std::fs::remove_file(&old) {
            Ok(()) => log::info!("removed {} left by an update", old.display()),
            Err(e) => log::info!("kept {} (still in use?): {e}", old.display()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installs_only_when_away() {
        let away = Moment { idle_secs: IDLE_BEFORE_INSTALL, ..Default::default() };
        assert!(may_install(&away));
        assert!(!may_install(&Moment { idle_secs: IDLE_BEFORE_INSTALL - 1, ..Default::default() }), "still at the PC");
        assert!(!may_install(&Moment { display_required: true, ..away }), "watching a video");
        assert!(!may_install(&Moment { analyzing: true, ..away }), "a batch is being analyzed");
        let locked = Moment { locked: true, ..Default::default() };
        assert!(may_install(&locked), "locked counts right away");
        assert!(may_install(&Moment { display_required: true, ..locked }));
        assert!(!may_install(&Moment { analyzing: true, ..locked }));
    }

    #[test]
    fn prepares_once_per_version() {
        let mut s = InstallStatus::idle();
        assert!(should_prepare(&s, "0.3.0", 1000));
        s = InstallStatus { stage: Stage::Downloading, version: Some("0.3.0".into()), ..InstallStatus::idle() };
        assert!(!should_prepare(&s, "0.3.0", 1000));
        assert!(installs_itself(&s, "0.3.0"));
        s.stage = Stage::Ready;
        assert!(!should_prepare(&s, "0.3.0", 1000));
        assert!(should_prepare(&s, "0.3.1", 1000), "a newer release replaces the download");
        assert!(!should_prepare(&s, "v0.3.0", 1000) && installs_itself(&s, "v0.3.0"), "same version, other spelling");
        assert!(installs_itself(&s, "0.3.0") && !installs_itself(&s, "0.3.1"));
        s = InstallStatus { stage: Stage::Failed, version: Some("0.3.0".into()), failed_at: Some(1000), error: Some("x".into()), ..InstallStatus::idle() };
        assert!(!installs_itself(&s, "0.3.0"), "a failed download keeps the notification");
        assert!(!should_prepare(&s, "0.3.0", 1000 + 3600));
        assert!(should_prepare(&s, "0.3.0", 1000 + RETRY_AFTER_FAILURE));
        assert!(should_prepare(&s, "0.3.1", 1001));
        assert!(should_prepare(&s, "0.3.0", 10), "clock went backwards");
    }

    #[test]
    fn installer_restarts_minimized_or_not() {
        assert_eq!(installer_args(true), ["/R", "/ARGS", "--minimized"]);
        assert_eq!(installer_args(false), ["/R", "/ARGS"]);
    }

    fn names(d: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        v.sort();
        v
    }

    #[test]
    fn aside_names() {
        assert_eq!(aside_name(1_800_000_000, 0), "daytrace-mcp.exe.1800000000.old");
        assert_eq!(aside_name(1_800_000_000, 2), "daytrace-mcp.exe.1800000000-2.old");
        assert_eq!(aside_stamp("daytrace-mcp.exe.1800000000.old"), Some((1_800_000_000, 0)));
        assert_eq!(aside_stamp("Daytrace-MCP.exe.1800000000-2.OLD"), Some((1_800_000_000, 2)));
        assert_eq!(aside_stamp("daytrace-mcp.exe.old"), Some((0, 0)), "the name 0.3.0 used");
        for other in ["daytrace-mcp.exe", "daytrace.exe.old", "daytrace-mcp.exe.x.old", "daytrace-mcp.exe.1.2.old", "daytrace-mcp.exe..old", "notes.old"] {
            assert_eq!(aside_stamp(other), None, "{other}");
        }
    }

    #[test]
    fn moves_the_mcp_server_aside_and_back() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let t = 1_800_000_000;
        assert_eq!(move_mcp_aside(d, t).unwrap(), None, "nothing to move");
        std::fs::write(d.join(MCP_EXE), "old").unwrap();
        let aside = move_mcp_aside(d, t).unwrap().unwrap();
        assert_eq!(aside, d.join("daytrace-mcp.exe.1800000000.old"));
        assert!(!d.join(MCP_EXE).exists());
        // The installer did not write a new one (or could not be started): the old one comes back.
        tidy_mcp_after_update(d);
        assert_eq!(std::fs::read_to_string(d.join(MCP_EXE)).unwrap(), "old");
        assert_eq!(names(d), [MCP_EXE]);
        // The installer wrote a new one: the old copy goes.
        move_mcp_aside(d, t).unwrap();
        std::fs::write(d.join(MCP_EXE), "new").unwrap();
        tidy_mcp_after_update(d);
        assert_eq!(std::fs::read_to_string(d.join(MCP_EXE)).unwrap(), "new");
        assert_eq!(names(d), [MCP_EXE]);
    }

    #[test]
    fn every_update_moves_to_a_name_of_its_own() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        // A copy from 0.3.0 and one from an earlier update are still there (maybe running).
        std::fs::write(d.join("daytrace-mcp.exe.old"), "0.3.0").unwrap();
        std::fs::write(d.join("daytrace-mcp.exe.1700000000.old"), "older").unwrap();
        std::fs::write(d.join(MCP_EXE), "current").unwrap();
        let first = move_mcp_aside(d, 1_800_000_000).unwrap().unwrap();
        assert_eq!(std::fs::read_to_string(&first).unwrap(), "current");
        assert_eq!(std::fs::read_to_string(d.join("daytrace-mcp.exe.old")).unwrap(), "0.3.0", "an older copy is left alone");
        // A second update in the same second gets the next name.
        std::fs::write(d.join(MCP_EXE), "next").unwrap();
        let second = move_mcp_aside(d, 1_800_000_000).unwrap().unwrap();
        assert_eq!(second, d.join("daytrace-mcp.exe.1800000000-1.old"));
        // Nothing was installed: the newest copy comes back, the rest goes.
        tidy_mcp_after_update(d);
        assert_eq!(std::fs::read_to_string(d.join(MCP_EXE)).unwrap(), "next");
        assert_eq!(names(d), [MCP_EXE]);
        // Other files in the folder stay.
        std::fs::write(d.join("daytrace.exe"), "app").unwrap();
        std::fs::write(d.join("notes.old"), "keep").unwrap();
        tidy_mcp_after_update(d);
        assert_eq!(names(d), ["daytrace-mcp.exe", "daytrace.exe", "notes.old"]);
    }
}
