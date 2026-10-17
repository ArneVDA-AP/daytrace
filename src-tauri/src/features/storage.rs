//! Storage: disk usage, screenshot purges, the screenshot size limit, database backups and the
//! agent (MCP) setup text for Settings → Storage.
//!
//! Ported from Dayflow (MIT, (c) 2025 Jerry Liu): Core/Recording/StorageManager+Maintenance.swift
//! (purge by size, oldest first, hourly; daily backups; integrity check and recovery, see
//! `backup`), Views/UI/Settings/StorageSettingsViewModel.swift and SettingsStorageTabView.swift
//! (usage, the bytes per hour measured over the last hour, size limit, open folder),
//! Core/Recording/StoragePreferences.swift (limit in bytes, 1 GB = 10^9 bytes) and
//! Core/AgentAccess/AgentClientRegistration.swift (the config snippet for MCP clients).
//! Dayflow purges by size only; Daytrace keeps its retention in days and adds the size limit
//! (`storage_limit_gb`, 0 = off). Timeline cards and observations are never touched here.

pub mod backup;

use crate::state::AppState;
use crate::timeutil::{logical_day, logical_day_bounds, now_ts};
use anyhow::{anyhow, Result};
use backup::{BackupInfo, DbHealth};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// Tables owned by this feature: none (backups are files, the limit is a setting).
pub const SCHEMA: &str = "";

/// How often the size limit is enforced.
const LIMIT_TICK: Duration = Duration::from_secs(5 * 60);
/// How often the loop checks whether a daily backup is due.
const BACKUP_CHECK_SECS: i64 = 3600;
/// Images from the last hour are never purged: they may not be batched or analyzed yet.
pub const KEEP_RECENT_SECS: i64 = 3600;
pub const GB: i64 = 1_000_000_000;

type S<'a> = tauri::State<'a, Arc<AppState>>;
type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

pub fn backups_dir(state: &AppState) -> PathBuf {
    state.paths.root.join(backup::BACKUP_DIR)
}

/// Background loop: a backup when the newest is a day old (checked hourly) and the size limit
/// every five minutes. Started from lib.rs.
pub fn spawn(state: Arc<AppState>) {
    tauri::async_runtime::spawn(async move {
        // Let startup (and the first analysis tick) settle first.
        tokio::time::sleep(Duration::from_secs(120)).await;
        let mut last_backup_check = i64::MIN / 2;
        loop {
            let check_backup = now_ts() - last_backup_check >= BACKUP_CHECK_SECS;
            if check_backup {
                last_backup_check = now_ts();
            }
            let st = state.clone();
            let run = tauri::async_runtime::spawn_blocking(move || {
                if check_backup {
                    if let Err(e) = backup::backup_if_due(&st.paths.db, &backups_dir(&st), now_ts()) {
                        log::error!("database backup failed: {e:#}");
                    }
                }
                if let Err(e) = enforce_limit(&st) {
                    log::error!("storage limit failed: {e:#}");
                }
            });
            if let Err(e) = run.await {
                log::error!("storage task panicked: {e}");
            }
            tokio::time::sleep(LIMIT_TICK).await;
        }
    });
}

// ---- purges ----

#[derive(Debug, Clone, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PurgeResult {
    pub files_removed: usize,
    pub bytes_freed: i64,
}

/// Deletes the image files of purged rows (only inside the screenshots folder) and then the
/// day folders left empty.
fn remove_files(screenshots: &Path, items: &[(String, i64)]) -> PurgeResult {
    let mut res = PurgeResult::default();
    for (p, _) in items {
        let path = Path::new(p);
        if p.is_empty() || !path.starts_with(screenshots) {
            continue;
        }
        let len = std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0);
        if std::fs::remove_file(path).is_ok() {
            res.files_removed += 1;
            res.bytes_freed += len;
        }
    }
    if let Ok(entries) = std::fs::read_dir(screenshots) {
        for e in entries.flatten() {
            let _ = std::fs::remove_dir(e.path()); // only succeeds when empty
        }
    }
    res
}

fn keep_after() -> i64 {
    now_ts() - KEEP_RECENT_SECS
}

/// Deletes screenshots older than `days` days now (0: all but the last hour).
pub fn delete_older_than(state: &AppState, days: u32) -> Result<PurgeResult> {
    let before = now_ts() - days as i64 * 86400;
    let items = state.db.expire_screenshots_between(i64::MIN, before, keep_after())?;
    let res = remove_files(&state.paths.screenshots, &items);
    log::info!("storage: deleted {} screenshots older than {days} days ({} bytes)", res.files_removed, res.bytes_freed);
    Ok(res)
}

/// Deletes every screenshot of one Daytrace day (4 AM to 4 AM).
pub fn delete_day(state: &AppState, day: &str) -> Result<PurgeResult> {
    let (from, to) = logical_day_bounds(day).ok_or_else(|| anyhow!("\"{day}\" is not a day (YYYY-MM-DD)"))?;
    let items = state.db.expire_screenshots_between(from, to, keep_after())?;
    let res = remove_files(&state.paths.screenshots, &items);
    log::info!("storage: deleted {} screenshots of {day} ({} bytes)", res.files_removed, res.bytes_freed);
    Ok(res)
}

/// Brings screenshots under `limit` bytes by deleting the oldest first
/// (Dayflow's performPurgeIfNeeded).
pub fn enforce_limit_bytes(state: &AppState, limit: i64) -> Result<PurgeResult> {
    let total = state.db.live_screenshot_bytes()?;
    if total <= limit {
        return Ok(PurgeResult::default());
    }
    let items = state.db.expire_oldest_screenshots(total - limit, keep_after())?;
    let res = remove_files(&state.paths.screenshots, &items);
    log::info!("storage: screenshots took {total} bytes, limit {limit}; deleted the oldest {} ({} bytes)", res.files_removed, res.bytes_freed);
    Ok(res)
}

/// Applies `storage_limit_gb` from the settings (nothing when it is 0).
pub fn enforce_limit(state: &AppState) -> Result<PurgeResult> {
    let gb = state.settings().storage_limit_gb as i64;
    if gb == 0 {
        return Ok(PurgeResult::default());
    }
    enforce_limit_bytes(state, gb * GB)
}

// ---- usage ----

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DayUsage {
    pub day: String,
    pub screenshots: i64,
    pub bytes: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageUsage {
    pub data_dir: String,
    /// Everything in the screenshots folder, measured on disk.
    pub screenshots_bytes: u64,
    pub screenshot_count: i64,
    /// Per Daytrace day, newest first, from the sizes recorded at capture.
    pub days: Vec<DayUsage>,
    /// daytrace.sqlite with its -wal and -shm files.
    pub database_bytes: u64,
    pub backups_bytes: u64,
    /// Newest first.
    pub backups: Vec<BackupInfo>,
    pub logs_bytes: u64,
    /// Measured over the last hour of captures (Dayflow's observedRecordingBytesPerHour); null
    /// until at least 10 minutes were recorded in that hour.
    pub bytes_per_hour: Option<i64>,
    /// Average per recorded day, today left out; null before the first full day.
    pub average_day_bytes: Option<i64>,
    pub health: DbHealth,
}

fn file_len(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

fn dir_size(p: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(p) else { return 0 };
    entries
        .flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

/// Groups `(captured_at, size)` rows by Daytrace day, newest day first.
pub fn per_day(rows: &[(i64, i64)]) -> Vec<DayUsage> {
    let mut m: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    for (ts, size) in rows {
        let e = m.entry(logical_day(*ts)).or_default();
        e.0 += 1;
        e.1 += size;
    }
    m.into_iter().rev().map(|(day, (n, b))| DayUsage { day, screenshots: n, bytes: b }).collect()
}

/// Bytes per hour over the captures since `since` (span of at least 10 minutes).
pub fn observed_bytes_per_hour(rows: &[(i64, i64)], since: i64) -> Option<i64> {
    let recent: Vec<&(i64, i64)> = rows.iter().filter(|(t, _)| *t >= since).collect();
    let first = recent.iter().map(|r| r.0).min()?;
    let last = recent.iter().map(|r| r.0).max()?;
    let span = last - first;
    if span < 600 {
        return None;
    }
    let bytes: i64 = recent.iter().map(|r| r.1).sum();
    Some((bytes as f64 * 3600.0 / span as f64) as i64)
}

pub fn usage(state: &AppState) -> Result<StorageUsage> {
    let rows = state.db.live_screenshot_sizes()?;
    let days = per_day(&rows);
    let today = logical_day(now_ts());
    let full: Vec<&DayUsage> = days.iter().filter(|d| d.day != today).collect();
    let average_day_bytes = (!full.is_empty()).then(|| full.iter().map(|d| d.bytes).sum::<i64>() / full.len() as i64);
    let db = &state.paths.db;
    let sidecar = |s: &str| PathBuf::from(format!("{}{s}", db.display()));
    let backups: Vec<BackupInfo> = backup::list_backups(&backups_dir(state)).into_iter().map(|(_, i)| i).collect();
    Ok(StorageUsage {
        data_dir: state.paths.root.to_string_lossy().to_string(),
        screenshots_bytes: dir_size(&state.paths.screenshots),
        screenshot_count: rows.len() as i64,
        days,
        database_bytes: file_len(db) + file_len(&sidecar("-wal")) + file_len(&sidecar("-shm")),
        backups_bytes: backups.iter().map(|b| b.bytes).sum(),
        backups,
        logs_bytes: file_len(&state.paths.root.join("daytrace.log")),
        bytes_per_hour: observed_bytes_per_hour(&rows, now_ts() - 3600),
        average_day_bytes,
        health: backup::health(),
    })
}

// ---- agent (MCP) setup ----

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSetup {
    /// daytrace-mcp next to the running app.
    pub exe_path: String,
    pub installed: bool,
    pub claude_code_command: String,
    /// The entry to put in claude_desktop_config.json.
    pub desktop_config: String,
    pub desktop_config_path: String,
}

pub fn agent_setup_for(exe: &Path) -> AgentSetup {
    let path = exe.to_string_lossy().to_string();
    let desktop = serde_json::json!({"mcpServers": {"daytrace": {"command": path, "args": []}}});
    AgentSetup {
        installed: exe.is_file(),
        claude_code_command: format!("claude mcp add --scope user --transport stdio daytrace -- \"{path}\""),
        desktop_config: serde_json::to_string_pretty(&desktop).unwrap_or_default(),
        desktop_config_path: if cfg!(windows) { "%APPDATA%\\Claude\\claude_desktop_config.json".into() } else { "claude_desktop_config.json".into() },
        exe_path: path,
    }
}

fn mcp_exe() -> PathBuf {
    let name = if cfg!(windows) { "daytrace-mcp.exe" } else { "daytrace-mcp" };
    std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(name))).unwrap_or_else(|| PathBuf::from(name))
}

// ---- commands ----

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> CmdResult<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(err)?.map_err(err)
}

#[tauri::command]
pub async fn storage_usage(state: S<'_>) -> CmdResult<StorageUsage> {
    let st = state.inner().clone();
    blocking(move || usage(&st)).await
}

#[tauri::command]
pub async fn storage_delete_older_than(state: S<'_>, days: u32) -> CmdResult<PurgeResult> {
    let st = state.inner().clone();
    blocking(move || delete_older_than(&st, days.min(3650))).await
}

#[tauri::command]
pub async fn storage_delete_day(state: S<'_>, day: String) -> CmdResult<PurgeResult> {
    let st = state.inner().clone();
    blocking(move || delete_day(&st, &day)).await
}

#[tauri::command]
pub async fn storage_backup_now(state: S<'_>) -> CmdResult<BackupInfo> {
    let st = state.inner().clone();
    blocking(move || backup::create_backup(&st.paths.db, &backups_dir(&st))).await
}

#[tauri::command]
pub fn storage_health() -> DbHealth {
    backup::health()
}

#[tauri::command]
pub fn storage_agent_setup() -> AgentSetup {
    agent_setup_for(&mcp_exe())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Paths;

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let st = AppState::new(Paths::new(dir.path().to_path_buf()).unwrap()).unwrap();
        (dir, st)
    }

    /// A screenshot file of `size` bytes in its day folder, with its row.
    fn shot(st: &AppState, ts: i64, size: usize) -> PathBuf {
        let dir = st.paths.screenshots.join(crate::timeutil::local(ts).format("%Y-%m-%d").to_string());
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{ts}.jpg"));
        std::fs::write(&path, vec![1u8; size]).unwrap();
        st.db.insert_screenshot(ts, &path.to_string_lossy(), size as i64, 0, Some("app"), None, None, None).unwrap();
        path
    }

    #[test]
    fn accounting_per_day_and_rates() {
        let (_d, st) = state();
        let (d1, _) = logical_day_bounds("2026-03-10").unwrap();
        let (d2, _) = logical_day_bounds("2026-03-11").unwrap();
        shot(&st, d1 + 3600, 100);
        shot(&st, d1 + 7200, 300);
        shot(&st, d2 + 3600, 50);
        // Rows without an image (idle, private app) do not count.
        st.db.insert_screenshot(d2 + 3700, "", 0, 0, None, None, Some("KeePass.exe"), None).unwrap();
        let u = usage(&st).unwrap();
        assert_eq!(u.screenshot_count, 3);
        assert_eq!(
            u.days,
            vec![
                DayUsage { day: "2026-03-11".into(), screenshots: 1, bytes: 50 },
                DayUsage { day: "2026-03-10".into(), screenshots: 2, bytes: 400 },
            ]
        );
        assert_eq!(u.screenshots_bytes, 450);
        assert!(u.database_bytes > 0);
        assert_eq!(u.average_day_bytes, Some(225));
        assert_eq!(u.backups_bytes, 0);

        let now = 1_000_000;
        assert_eq!(observed_bytes_per_hour(&[(now - 300, 10), (now, 10)], now - 3600), None, "under 10 minutes");
        assert_eq!(observed_bytes_per_hour(&[(now - 1800, 100), (now - 900, 100), (now, 100)], now - 3600), Some(600));
    }

    #[test]
    fn size_limit_purges_oldest_first_and_keeps_protected_images() {
        let (_d, st) = state();
        let now = now_ts();
        let old: Vec<PathBuf> = (0..5).map(|i| shot(&st, now - 10 * 86400 + i * 60, 100)).collect();
        // In a batch that still waits for analysis: kept even though old.
        let waiting = shot(&st, now - 9 * 86400, 100);
        let rows = st.db.screenshots_in_range(now - 9 * 86400, now - 9 * 86400 + 1).unwrap();
        st.db.create_batch(&rows).unwrap();
        // From the last hour: kept.
        let recent = shot(&st, now - 60, 100);
        assert_eq!(st.db.live_screenshot_bytes().unwrap(), 700);

        let res = enforce_limit_bytes(&st, 450).unwrap();
        assert_eq!(res, PurgeResult { files_removed: 3, bytes_freed: 300 });
        assert!(!old[0].exists() && !old[1].exists() && !old[2].exists());
        assert!(old[3].exists() && old[4].exists() && waiting.exists() && recent.exists());
        assert_eq!(st.db.live_screenshot_bytes().unwrap(), 400);
        // Already under the limit: nothing happens.
        assert_eq!(enforce_limit_bytes(&st, 450).unwrap(), PurgeResult::default());
        // Even a tiny limit never touches protected images.
        enforce_limit_bytes(&st, 0).unwrap();
        assert!(waiting.exists() && recent.exists());
        assert!(!old[4].exists());
        // The setting: 0 means off.
        assert_eq!(enforce_limit(&st).unwrap(), PurgeResult::default());
        // Cards and observations stay; rows are only soft-deleted.
        assert_eq!(st.db.screenshots_in_range(0, i64::MAX).unwrap().len(), 2);
    }

    #[test]
    fn delete_older_than_and_by_day() {
        let (_d, st) = state();
        let now = now_ts();
        let a = shot(&st, now - 20 * 86400, 10);
        let b = shot(&st, now - 3 * 86400, 10);
        let c = shot(&st, now - 2 * 86400, 10);
        let res = delete_older_than(&st, 7).unwrap();
        assert_eq!(res.files_removed, 1);
        assert!(!a.exists() && b.exists());
        assert!(!a.parent().unwrap().exists(), "the day folder left empty is removed");
        let day = logical_day(now - 3 * 86400);
        let res = delete_day(&st, &day).unwrap();
        assert_eq!(res.files_removed, 1);
        assert!(!b.exists() && c.exists());
        assert!(delete_day(&st, "yesterday-ish").is_err());
        // Paths outside the screenshots folder are never removed.
        let outside = st.paths.root.join("keep.jpg");
        std::fs::write(&outside, b"x").unwrap();
        st.db.insert_screenshot(now - 30 * 86400, &outside.to_string_lossy(), 1, 0, None, None, None, None).unwrap();
        delete_older_than(&st, 7).unwrap();
        assert!(outside.exists());
    }

    #[test]
    fn agent_setup_text() {
        let s = agent_setup_for(Path::new("C:\\Users\\me\\AppData\\Local\\Daytrace\\daytrace-mcp.exe"));
        assert!(!s.installed);
        assert_eq!(s.claude_code_command, "claude mcp add --scope user --transport stdio daytrace -- \"C:\\Users\\me\\AppData\\Local\\Daytrace\\daytrace-mcp.exe\"");
        let v: serde_json::Value = serde_json::from_str(&s.desktop_config).unwrap();
        assert_eq!(v["mcpServers"]["daytrace"]["command"], "C:\\Users\\me\\AppData\\Local\\Daytrace\\daytrace-mcp.exe");
    }
}
