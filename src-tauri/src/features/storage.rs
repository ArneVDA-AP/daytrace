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
        // After a restore or reset at startup, the images captured since the restored backup (or
        // all of them) have no rows; once a backup newer than them exists they are swept.
        let recovered = matches!(backup::health().status, backup::HealthStatus::Restored | backup::HealthStatus::Reset);
        let mut swept_after_recovery = !recovered;
        loop {
            let check_backup = now_ts() - last_backup_check >= BACKUP_CHECK_SECS;
            if check_backup {
                last_backup_check = now_ts();
            }
            let st = state.clone();
            let sweep = !swept_after_recovery;
            let run = tauri::async_runtime::spawn_blocking(move || {
                if check_backup {
                    if let Err(e) = backup::backup_if_due(&st.paths.db, &backups_dir(&st), now_ts()) {
                        log::error!("database backup failed: {e:#}");
                    }
                }
                if sweep {
                    // A fresh backup first: files older than it and without a row are strays.
                    if let Err(e) = backup::create_backup(&st.paths.db, &backups_dir(&st)) {
                        log::error!("database backup after recovery failed: {e:#}");
                    }
                    let cutoff = now_ts() - st.settings().retention_days as i64 * 86400;
                    sweep_screenshot_files(&st, cutoff);
                }
                if let Err(e) = enforce_limit(&st) {
                    log::error!("storage limit failed: {e:#}");
                }
            });
            swept_after_recovery = true;
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

// ---- files the database does not account for ----

/// A day folder is deleted whole once its day ended this long before the retention cutoff (the
/// extra day covers a time-zone change between capture and now).
const DAY_FOLDER_MARGIN_SECS: i64 = 86400;
/// Damaged database files moved aside by the startup check are kept this long.
pub const DAMAGED_KEEP_DAYS: i64 = 30;

/// `"YYYY-MM-DD/name.jpg"`: a screenshot path by its day folder and file name, so a data folder
/// that moved still matches its rows.
fn file_key(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    let day = path.parent()?.file_name()?.to_str()?;
    Some(format!("{day}/{name}"))
}

/// Screenshot files the rows do not account for. Retention and the size limit work from the
/// rows; after a database restore (images captured after the backup have no row), a reset (no
/// image has one) or a failed delete, files stay on disk that neither ever sees.
/// - A day folder (`screenshots/YYYY-MM-DD`, the capture's local date) whose day ended more than a
///   day before `cutoff` (the retention cutoff) is deleted whole, whatever the rows say.
/// - In the other folders, a file that no live row points to is deleted when it is older than
///   the newest database backup: a capture writes its row right after its file, so such a file
///   would have had its row in that backup and since.
///
/// Returns the number of files removed.
pub fn sweep_files(dir: &Path, db: &crate::db::Db, cutoff: i64, newest_backup: Option<i64>) -> Result<usize> {
    let mut removed = 0;
    let mut referenced: Option<std::collections::HashSet<String>> = None;
    let midnight = |d: chrono::NaiveDate| crate::timeutil::local_at(d, chrono::NaiveTime::MIN);
    for entry in std::fs::read_dir(dir)?.flatten() {
        let folder = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_string) else { continue };
        let Ok(date) = chrono::NaiveDate::parse_from_str(&name, "%Y-%m-%d") else { continue };
        if !folder.is_dir() {
            continue;
        }
        let (Some(start), Some(end)) = (midnight(date), date.succ_opt().and_then(midnight)) else { continue };
        if end + DAY_FOLDER_MARGIN_SECS <= cutoff {
            let files = std::fs::read_dir(&folder).map(|r| r.flatten().count()).unwrap_or(0);
            if std::fs::remove_dir_all(&folder).is_ok() {
                removed += files;
            }
            continue;
        }
        // Stray files: only in folders that can hold files from before the newest backup.
        let Some(backup_at) = newest_backup.filter(|b| start < *b) else { continue };
        if referenced.is_none() {
            let c = db.conn();
            let mut st = c.prepare("SELECT file_path FROM screenshots WHERE is_deleted = 0 AND file_path != ''")?;
            let paths = st.query_map([], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            referenced = Some(paths.iter().filter_map(|p| file_key(Path::new(p))).collect());
        }
        let known = referenced.as_ref().unwrap();
        for f in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let path = f.path();
            if !path.is_file() || file_key(&path).is_some_and(|k| known.contains(&k)) {
                continue;
            }
            let modified = f.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64);
            if modified.is_some_and(|m| m < backup_at) && std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
        }
        let _ = std::fs::remove_dir(&folder); // only succeeds when empty
    }
    Ok(removed)
}

/// `sweep_files` over the app's screenshots folder, against its newest backup. Called by the
/// hourly retention and once after a database restore or reset.
pub fn sweep_screenshot_files(state: &AppState, cutoff: i64) -> usize {
    let newest = backup::list_backups(&backups_dir(state)).into_iter().map(|(_, i)| i.created_at).max();
    match sweep_files(&state.paths.screenshots, &state.db, cutoff, newest) {
        Ok(n) => {
            if n > 0 {
                log::info!("storage: removed {n} screenshot files the database had no current row for");
            }
            n
        }
        Err(e) => {
            log::warn!("storage: sweeping screenshot files failed: {e:#}");
            0
        }
    }
}

/// Deletes `daytrace-damaged-*.sqlite` files (and their `-wal`) in the data folder that were moved
/// aside more than `DAMAGED_KEEP_DAYS` ago (by the time in the name, else the file's date). They
/// hold old observations and window titles, so they do not stay forever.
pub fn remove_old_damaged_databases(root: &Path, now: i64) -> usize {
    let mut removed = 0;
    for e in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        let Some(rest) = name.strip_prefix("daytrace-damaged-") else { continue };
        let stamp = rest.split(".sqlite").next().unwrap_or("");
        let at = chrono::NaiveDateTime::parse_from_str(stamp, "%Y-%m-%d_%H%M%S")
            .ok()
            .and_then(|t| crate::timeutil::local_at(t.date(), t.time()))
            .or_else(|| e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64));
        if at.is_some_and(|t| now - t > DAMAGED_KEEP_DAYS * 86400) && e.path().is_file() && std::fs::remove_file(e.path()).is_ok() {
            log::info!("removed the damaged database copy {name}");
            removed += 1;
        }
    }
    removed
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

/// Slices for per-day accounting: 15 minutes, so every UTC offset in use (down to +5:45) puts
/// the 4 AM day boundary on a slice edge.
const BUCKET_SECS: i64 = 900;

/// Groups `(slice start, count, bytes)` rows by Daytrace day, newest day first.
pub fn per_day(buckets: &[(i64, i64, i64)]) -> Vec<DayUsage> {
    let mut m: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    for (ts, n, bytes) in buckets {
        let e = m.entry(logical_day(*ts)).or_default();
        e.0 += n;
        e.1 += bytes;
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
    let days = per_day(&state.db.live_screenshot_buckets(BUCKET_SECS)?);
    let since = now_ts() - 3600;
    let recent = state.db.live_screenshot_sizes_since(since)?;
    let today = logical_day(now_ts());
    let full: Vec<&DayUsage> = days.iter().filter(|d| d.day != today).collect();
    let average_day_bytes = (!full.is_empty()).then(|| full.iter().map(|d| d.bytes).sum::<i64>() / full.len() as i64);
    let db = &state.paths.db;
    let sidecar = |s: &str| PathBuf::from(format!("{}{s}", db.display()));
    let backups: Vec<BackupInfo> = backup::list_backups(&backups_dir(state)).into_iter().map(|(_, i)| i).collect();
    Ok(StorageUsage {
        data_dir: state.paths.root.to_string_lossy().to_string(),
        screenshots_bytes: dir_size(&state.paths.screenshots),
        screenshot_count: days.iter().map(|d| d.screenshots).sum(),
        days,
        database_bytes: file_len(db) + file_len(&sidecar("-wal")) + file_len(&sidecar("-shm")),
        backups_bytes: backups.iter().map(|b| b.bytes).sum(),
        backups,
        logs_bytes: file_len(&state.paths.root.join("daytrace.log")),
        bytes_per_hour: observed_bytes_per_hour(&recent, since),
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
        // 3:59:30 AM belongs to the day before, 4:00:00 AM to the new day.
        shot(&st, d2 - 30, 7);
        shot(&st, d2, 3);
        // Rows without an image (idle, private app) do not count.
        st.db.insert_screenshot(d2 + 3700, "", 0, 0, None, None, Some("KeePass.exe"), None).unwrap();
        let u = usage(&st).unwrap();
        assert_eq!(u.screenshot_count, 5);
        assert_eq!(
            u.days,
            vec![
                DayUsage { day: "2026-03-11".into(), screenshots: 2, bytes: 53 },
                DayUsage { day: "2026-03-10".into(), screenshots: 3, bytes: 407 },
            ]
        );
        assert_eq!(u.screenshots_bytes, 460);
        assert!(u.database_bytes > 0);
        assert_eq!(u.average_day_bytes, Some(230));
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

    fn set_mtime(p: &Path, ts: i64) {
        let f = std::fs::File::options().write(true).open(p).unwrap();
        f.set_modified(std::time::UNIX_EPOCH + Duration::from_secs(ts as u64)).unwrap();
    }

    #[test]
    fn sweep_removes_files_the_database_does_not_know() {
        let (_d, st) = state();
        let now = now_ts();
        let cutoff = now - 7 * 86400;
        // A day past the retention period, with a row that is still live (as after a restore):
        // the folder goes whole.
        let old = shot(&st, now - 20 * 86400, 10);
        // Two days ago: a file with its row, a stray older than the newest backup (captured after
        // the backup that was restored), and a stray newer than that backup.
        let known = shot(&st, now - 2 * 86400, 10);
        set_mtime(&known, now - 2 * 86400);
        let day = known.parent().unwrap().to_path_buf();
        let stray = day.join("235959_999.jpg");
        std::fs::write(&stray, b"x").unwrap();
        set_mtime(&stray, now - 2 * 86400);
        let fresh = day.join("235958_999.jpg");
        std::fs::write(&fresh, b"x").unwrap();
        // Not a day folder: left alone.
        let other = st.paths.screenshots.join("notes");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("a.txt"), b"x").unwrap();

        // Without a backup only the old day goes.
        assert_eq!(sweep_files(&st.paths.screenshots, &st.db, cutoff, None).unwrap(), 1);
        assert!(!old.exists() && !old.parent().unwrap().exists());
        assert!(stray.exists());
        let backup_at = now - 86400;
        assert_eq!(sweep_files(&st.paths.screenshots, &st.db, cutoff, Some(backup_at)).unwrap(), 1);
        assert!(!stray.exists());
        assert!(known.exists() && fresh.exists() && other.join("a.txt").exists());
        // A file whose row was purged (path cleared) but that is still there counts as a stray.
        st.db.expire_screenshots_between(now - 3 * 86400, now - 86400, i64::MAX).unwrap();
        assert_eq!(sweep_files(&st.paths.screenshots, &st.db, cutoff, Some(backup_at)).unwrap(), 1);
        assert!(!known.exists() && fresh.exists());
    }

    #[test]
    fn old_damaged_databases_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let stamp_at = crate::timeutil::local_at(chrono::NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(), chrono::NaiveTime::from_hms_opt(10, 10, 10).unwrap()).unwrap();
        for name in ["daytrace-damaged-2026-08-01_101010.sqlite", "daytrace-damaged-2026-08-01_101010.sqlite-wal", "daytrace-damaged-2026-08-25_090000.sqlite", "daytrace.sqlite"] {
            std::fs::write(dir.path().join(name), b"x").unwrap();
        }
        let now = stamp_at + 31 * 86400;
        assert_eq!(remove_old_damaged_databases(dir.path(), now), 2);
        let mut left: Vec<String> = std::fs::read_dir(dir.path()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect();
        left.sort();
        assert_eq!(left, vec!["daytrace-damaged-2026-08-25_090000.sqlite".to_string(), "daytrace.sqlite".to_string()]);
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
