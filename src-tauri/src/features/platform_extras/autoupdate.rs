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

/// Whether to start downloading `latest` now.
pub fn should_prepare(status: &InstallStatus, latest: &str, now: i64) -> bool {
    let same = status.version.as_deref() == Some(latest);
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
    matches!(status.stage, Stage::Downloading | Stage::Ready | Stage::Installing) && status.version.as_deref() == Some(version)
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
const MCP_ASIDE: &str = "daytrace-mcp.exe.old";

/// Before the installer runs: moves `daytrace-mcp.exe` aside so the installer can write the new
/// one. Running copies keep working from the renamed file. Returns whether it was moved.
pub fn move_mcp_aside(dir: &Path) -> std::io::Result<bool> {
    let exe = dir.join(MCP_EXE);
    if !exe.is_file() {
        return Ok(false);
    }
    let aside = dir.join(MCP_ASIDE);
    // A copy left from an earlier update that is no longer running.
    let _ = std::fs::remove_file(&aside);
    std::fs::rename(&exe, &aside)?;
    Ok(true)
}

/// At startup: removes the copy moved aside by an update, or puts it back when the update did
/// not install a new one. A copy still running stays until a later start.
pub fn tidy_mcp_after_update(dir: &Path) {
    let aside = dir.join(MCP_ASIDE);
    if !aside.is_file() {
        return;
    }
    let exe = dir.join(MCP_EXE);
    if exe.is_file() {
        if std::fs::remove_file(&aside).is_ok() {
            log::info!("removed {MCP_ASIDE} left by the update");
        }
    } else if let Err(e) = std::fs::rename(&aside, &exe) {
        log::warn!("could not put {MCP_EXE} back: {e}");
    } else {
        log::info!("the update did not install {MCP_EXE}; put the previous one back");
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

    #[test]
    fn moves_the_mcp_server_aside_and_back() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        assert!(!move_mcp_aside(d).unwrap(), "nothing to move");
        std::fs::write(d.join(MCP_EXE), "old").unwrap();
        assert!(move_mcp_aside(d).unwrap());
        assert!(!d.join(MCP_EXE).exists() && d.join(MCP_ASIDE).exists());
        // The installer did not write a new one: the old one comes back.
        tidy_mcp_after_update(d);
        assert_eq!(std::fs::read_to_string(d.join(MCP_EXE)).unwrap(), "old");
        assert!(!d.join(MCP_ASIDE).exists());
        // The installer wrote a new one: the old copy goes.
        assert!(move_mcp_aside(d).unwrap());
        std::fs::write(d.join(MCP_EXE), "new").unwrap();
        tidy_mcp_after_update(d);
        assert_eq!(std::fs::read_to_string(d.join(MCP_EXE)).unwrap(), "new");
        assert!(!d.join(MCP_ASIDE).exists());
        // A stale copy from an earlier update is replaced on the next move.
        std::fs::write(d.join(MCP_ASIDE), "stale").unwrap();
        assert!(move_mcp_aside(d).unwrap());
        assert_eq!(std::fs::read_to_string(d.join(MCP_ASIDE)).unwrap(), "new");
    }
}
