//! Database safety, ported from Dayflow's Core/Recording/StorageManager+Maintenance.swift (MIT,
//! (c) 2025 Jerry Liu): a daily backup made with SQLite's online backup API (Dayflow keeps 3;
//! Daytrace keeps 7), an integrity check (`PRAGMA quick_check`) before the database is opened,
//! and recovery from the newest good backup when the database is damaged. As in Dayflow, only
//! corruption triggers recovery: when the file cannot be opened for other reasons (locked,
//! permissions, disk full) it is left alone. Unlike Dayflow, the damaged file is moved aside and
//! kept instead of deleted.

use anyhow::{bail, Context, Result};
use chrono::Local;
use rusqlite::backup::{Backup, StepResult};
use rusqlite::{Connection, ErrorCode, OpenFlags};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, UNIX_EPOCH};

pub const BACKUP_DIR: &str = "backups";
/// Backups kept (newest first).
pub const KEEP: usize = 7;
/// A new backup is made when the newest one is older than this.
pub const BACKUP_INTERVAL_SECS: i64 = 24 * 3600;
const PREFIX: &str = "daytrace-";
const EXT: &str = ".sqlite";

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub name: String,
    pub bytes: u64,
    /// Unix seconds (file modification time).
    pub created_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    /// Not checked (tests, or the check has not run yet).
    Unchecked,
    /// The check passed, or there was no database yet.
    Ok,
    /// The database was damaged and the newest good backup was put in its place.
    Restored,
    /// The database was damaged, no backup was usable, and a new empty database was started.
    Reset,
    /// The database is damaged and could not be moved aside.
    Damaged,
}

/// What the startup check found; shown in Settings → Storage.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DbHealth {
    pub status: HealthStatus,
    /// The integrity check's message or the error, when something was wrong.
    pub detail: Option<String>,
    /// File name of the backup that was restored.
    pub restored_from: Option<String>,
    pub restored_backup_at: Option<i64>,
    /// File name the damaged database was moved to (in the data folder).
    pub moved_to: Option<String>,
    pub checked_at: i64,
}

impl DbHealth {
    fn new(status: HealthStatus) -> DbHealth {
        DbHealth { status, detail: None, restored_from: None, restored_backup_at: None, moved_to: None, checked_at: crate::timeutil::now_ts() }
    }
}

static HEALTH: Mutex<Option<DbHealth>> = Mutex::new(None);

/// Result of the startup check (Unchecked before it runs).
pub fn health() -> DbHealth {
    HEALTH.lock().unwrap_or_else(|e| e.into_inner()).clone().unwrap_or_else(|| DbHealth::new(HealthStatus::Unchecked))
}

/// Startup hook, called right before `Db::open`: checks the database and repairs it from a
/// backup when it is damaged. Never fails; problems are logged and kept for the UI.
pub fn check_database(root: &Path, db: &Path) {
    let h = startup_check(db, &root.join(BACKUP_DIR));
    match h.status {
        HealthStatus::Ok | HealthStatus::Unchecked => log::info!("database check passed"),
        HealthStatus::Restored => log::warn!("database was damaged ({:?}); restored backup {:?}; damaged file kept as {:?}", h.detail, h.restored_from, h.moved_to),
        HealthStatus::Reset => log::error!("database was damaged ({:?}) and no backup was usable; started a new database; damaged file kept as {:?}", h.detail, h.moved_to),
        HealthStatus::Damaged => log::error!("database is damaged and could not be moved aside: {:?}", h.detail),
    }
    *HEALTH.lock().unwrap_or_else(|e| e.into_inner()) = Some(h);
}

/// The outcome of `PRAGMA quick_check` on one file.
#[derive(Debug, Clone, PartialEq)]
pub enum FileCheck {
    Ok,
    /// Damaged: corruption error, "not a database", or quick_check reported problems.
    Corrupt(String),
    /// Could not be checked for reasons other than damage (locked, permissions, I/O).
    Unavailable(String),
}

fn is_corruption(e: &rusqlite::Error) -> bool {
    matches!(e.sqlite_error_code(), Some(ErrorCode::DatabaseCorrupt) | Some(ErrorCode::NotADatabase))
}

/// Runs `PRAGMA quick_check` on an existing database file without creating anything.
pub fn check_file(path: &Path) -> FileCheck {
    let classify = |e: rusqlite::Error| if is_corruption(&e) { FileCheck::Corrupt(e.to_string()) } else { FileCheck::Unavailable(e.to_string()) };
    let conn = match Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX) {
        Ok(c) => c,
        Err(e) => return classify(e),
    };
    let _ = conn.busy_timeout(Duration::from_secs(5));
    let rows: rusqlite::Result<Vec<String>> = conn.prepare("PRAGMA quick_check").and_then(|mut st| st.query_map([], |r| r.get::<_, String>(0))?.collect());
    match rows {
        Ok(r) if r.len() == 1 && r[0] == "ok" => FileCheck::Ok,
        Ok(r) => FileCheck::Corrupt(r.into_iter().take(5).collect::<Vec<_>>().join("; ")),
        Err(e) => classify(e),
    }
}

fn sidecar(db: &Path, suffix: &str) -> PathBuf {
    let mut s = db.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

/// Checks `db`; when it is damaged, moves it aside and restores the newest backup in `backups`
/// that passes the same check. Without a usable backup the app starts with a new database.
pub fn startup_check(db: &Path, backups: &Path) -> DbHealth {
    if !db.exists() {
        return DbHealth::new(HealthStatus::Ok);
    }
    let detail = match check_file(db) {
        FileCheck::Ok => return DbHealth::new(HealthStatus::Ok),
        FileCheck::Unavailable(e) => {
            // Not damage: leave the file alone; Db::open reports the problem.
            log::warn!("could not check the database: {e}");
            let mut h = DbHealth::new(HealthStatus::Unchecked);
            h.detail = Some(e);
            return h;
        }
        FileCheck::Corrupt(d) => d,
    };
    log::error!("database integrity check failed: {detail}");
    let mut h = DbHealth::new(HealthStatus::Damaged);
    h.detail = Some(detail);

    // Move the damaged file (and its WAL) aside; the shared-memory file is rebuilt by SQLite.
    let stamp = Local::now().format("%Y-%m-%d_%H%M%S");
    let aside = db.with_file_name(format!("daytrace-damaged-{stamp}{EXT}"));
    if let Err(e) = std::fs::rename(db, &aside) {
        h.detail = Some(format!("{} (could not move the file aside: {e})", h.detail.unwrap_or_default()));
        return h;
    }
    let wal = sidecar(db, "-wal");
    if wal.exists() {
        let _ = std::fs::rename(&wal, sidecar(&aside, "-wal"));
    }
    let _ = std::fs::remove_file(sidecar(db, "-shm"));
    h.moved_to = aside.file_name().map(|n| n.to_string_lossy().to_string());

    for (path, info) in list_backups(backups) {
        match check_file(&path) {
            FileCheck::Ok => {}
            other => {
                log::warn!("backup {} is not usable: {other:?}", info.name);
                continue;
            }
        }
        let tmp = sidecar(db, ".restoring");
        match std::fs::copy(&path, &tmp).and_then(|_| std::fs::rename(&tmp, db)) {
            Ok(()) => {
                h.status = HealthStatus::Restored;
                h.restored_from = Some(info.name);
                h.restored_backup_at = Some(info.created_at);
                return h;
            }
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                log::error!("restoring backup {} failed: {e}", info.name);
            }
        }
    }
    h.status = HealthStatus::Reset;
    h
}

fn mtime(path: &Path) -> i64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Backups in `dir`, newest first. Names carry a sortable timestamp
/// (daytrace-2026-10-17_031500.sqlite), so name order is age order.
pub fn list_backups(dir: &Path) -> Vec<(PathBuf, BackupInfo)> {
    let Ok(entries) = std::fs::read_dir(dir) else { return vec![] };
    let mut v: Vec<(PathBuf, BackupInfo)> = entries
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().to_string();
            if !(name.starts_with(PREFIX) && name.ends_with(EXT)) || !e.path().is_file() {
                return None;
            }
            let path = e.path();
            let bytes = e.metadata().map(|m| m.len()).unwrap_or(0);
            let created_at = mtime(&path);
            Some((path, BackupInfo { name, bytes, created_at }))
        })
        .collect();
    v.sort_by(|a, b| b.1.name.cmp(&a.1.name));
    v
}

/// Removes all but the newest `keep` backups.
pub fn prune(dir: &Path, keep: usize) -> usize {
    let mut removed = 0;
    for (path, info) in list_backups(dir).into_iter().skip(keep) {
        if std::fs::remove_file(&path).is_ok() {
            log::info!("removed old backup {}", info.name);
            removed += 1;
        }
    }
    removed
}

/// Copies the live database into `dir` with SQLite's online backup API, checks the copy, and
/// keeps the newest `KEEP` backups. Reads through its own read-only connection, so the app keeps
/// writing meanwhile (WAL); the copy is one consistent snapshot.
pub fn create_backup(db: &Path, dir: &Path) -> Result<BackupInfo> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let name = format!("{PREFIX}{}{EXT}", Local::now().format("%Y-%m-%d_%H%M%S"));
    let target = dir.join(&name);
    let tmp = dir.join(format!("{name}.partial"));
    let _ = std::fs::remove_file(&tmp);
    let result = (|| -> Result<()> {
        let src = Connection::open_with_flags(db, OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX)?;
        src.busy_timeout(Duration::from_secs(5))?;
        let mut dst = Connection::open(&tmp)?;
        {
            let backup = Backup::new(&src, &mut dst)?;
            let mut waits = 0;
            loop {
                // -1: all pages in one step, holding one read snapshot of the source.
                match backup.step(-1)? {
                    StepResult::Done => break,
                    StepResult::More => {}
                    _ => {
                        waits += 1;
                        if waits > 50 {
                            bail!("the database stayed busy");
                        }
                        std::thread::sleep(Duration::from_millis(200));
                    }
                }
            }
        }
        // A self-contained single file, whatever the source's journal mode.
        dst.query_row("PRAGMA journal_mode = DELETE", [], |_| Ok(()))?;
        drop(dst);
        match check_file(&tmp) {
            FileCheck::Ok => Ok(()),
            FileCheck::Corrupt(d) | FileCheck::Unavailable(d) => bail!("the backup copy failed its integrity check: {d}"),
        }
    })();
    if let Err(e) = result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    let _ = std::fs::remove_file(&target);
    std::fs::rename(&tmp, &target)?;
    prune(dir, KEEP);
    let bytes = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
    log::info!("database backup created: {name} ({bytes} bytes)");
    Ok(BackupInfo { name, bytes, created_at: mtime(&target) })
}

/// Makes a backup when there is none younger than `BACKUP_INTERVAL_SECS`.
pub fn backup_if_due(db: &Path, dir: &Path, now: i64) -> Result<Option<BackupInfo>> {
    if !db.exists() {
        return Ok(None);
    }
    let newest = list_backups(dir).into_iter().next().map(|(_, i)| i.created_at);
    if newest.is_some_and(|t| now - t < BACKUP_INTERVAL_SECS && t <= now) {
        return Ok(None);
    }
    create_backup(db, dir).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn db_with_rows(dir: &Path) -> PathBuf {
        let path = dir.join("daytrace.sqlite");
        let db = Db::open(&path).unwrap();
        for i in 0..50 {
            db.insert_screenshot(1_700_000_000 + i, "", 0, 0, Some("app"), Some("title"), None, None).unwrap();
        }
        path
    }

    fn count(path: &Path) -> i64 {
        Connection::open(path).unwrap().query_row("SELECT COUNT(*) FROM screenshots", [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn backup_copies_a_live_database() {
        let dir = tempfile::tempdir().unwrap();
        let path = db_with_rows(dir.path());
        // Keep a writer open, as the app does.
        let live = Db::open(&path).unwrap();
        let backups = dir.path().join(BACKUP_DIR);
        let info = create_backup(&path, &backups).unwrap();
        live.insert_screenshot(1_800_000_000, "", 0, 0, None, None, None, None).unwrap();
        let copy = backups.join(&info.name);
        assert_eq!(count(&copy), 50);
        assert_eq!(check_file(&copy), FileCheck::Ok);
        let mode: String = Connection::open(&copy).unwrap().query_row("PRAGMA journal_mode", [], |r| r.get(0)).unwrap();
        assert_eq!(mode, "delete");
        assert!(info.bytes > 0);
        // A fresh backup is not due again; an old one is.
        assert_eq!(backup_if_due(&path, &backups, info.created_at + 60).unwrap(), None);
        assert!(backup_if_due(&path, &backups, info.created_at + BACKUP_INTERVAL_SECS + 1).unwrap().is_some());
        assert!(!std::fs::read_dir(&backups).unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with(".partial")));
    }

    #[test]
    fn rotation_keeps_the_newest_seven() {
        let dir = tempfile::tempdir().unwrap();
        for d in 1..=9 {
            std::fs::write(dir.path().join(format!("daytrace-2026-10-{d:02}_030000.sqlite")), b"x").unwrap();
        }
        std::fs::write(dir.path().join("notes.txt"), b"keep me").unwrap();
        std::fs::write(dir.path().join("daytrace-2026-10-10_030000.sqlite.partial"), b"x").unwrap();
        assert_eq!(prune(dir.path(), KEEP), 2);
        let names: Vec<String> = list_backups(dir.path()).into_iter().map(|(_, i)| i.name).collect();
        assert_eq!(names.len(), 7);
        assert_eq!(names.first().unwrap(), "daytrace-2026-10-09_030000.sqlite");
        assert_eq!(names.last().unwrap(), "daytrace-2026-10-03_030000.sqlite");
        assert!(dir.path().join("notes.txt").exists());
    }

    #[test]
    fn garbage_database_is_restored_from_the_newest_good_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = db_with_rows(dir.path());
        let backups = dir.path().join(BACKUP_DIR);
        std::fs::create_dir_all(&backups).unwrap();
        // Older good backup, newer broken one.
        std::fs::copy(&path, backups.join("daytrace-2026-10-01_030000.sqlite")).unwrap();
        std::fs::write(backups.join("daytrace-2026-10-02_030000.sqlite"), vec![0x42u8; 8192]).unwrap();
        std::fs::write(&path, b"this is not a database at all, just garbage bytes".repeat(200)).unwrap();
        assert!(matches!(check_file(&path), FileCheck::Corrupt(_)));

        let h = startup_check(&path, &backups);
        assert_eq!(h.status, HealthStatus::Restored);
        assert_eq!(h.restored_from.as_deref(), Some("daytrace-2026-10-01_030000.sqlite"));
        let moved = dir.path().join(h.moved_to.unwrap());
        assert!(std::fs::read(&moved).unwrap().starts_with(b"this is not a database"), "damaged file kept");
        assert_eq!(check_file(&path), FileCheck::Ok);
        assert_eq!(count(&path), 50);
        // The app opens it normally afterwards.
        Db::open(&path).unwrap();
    }

    #[test]
    fn damaged_without_backup_starts_fresh_and_healthy_is_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("daytrace.sqlite");
        assert_eq!(startup_check(&path, &dir.path().join(BACKUP_DIR)).status, HealthStatus::Ok, "no database yet");
        let path = db_with_rows(dir.path());
        let before = std::fs::read(&path).unwrap();
        assert_eq!(startup_check(&path, &dir.path().join(BACKUP_DIR)).status, HealthStatus::Ok);
        assert_eq!(std::fs::read(&path).unwrap(), before);

        std::fs::write(&path, vec![0xFFu8; 4096]).unwrap();
        let h = startup_check(&path, &dir.path().join(BACKUP_DIR));
        assert_eq!(h.status, HealthStatus::Reset);
        assert!(h.detail.is_some());
        assert!(!path.exists(), "damaged file moved away");
        let db = Db::open(&path).unwrap();
        assert_eq!(db.last_capture_ts().unwrap(), None);
    }

    #[test]
    fn damaged_pages_are_caught_by_quick_check() {
        let dir = tempfile::tempdir().unwrap();
        let path = db_with_rows(dir.path());
        {
            // Fold the WAL into the main file so the damage below hits real pages.
            let c = Connection::open(&path).unwrap();
            c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).unwrap();
            c.query_row("PRAGMA journal_mode = DELETE", [], |_| Ok(())).unwrap();
        }
        let mut bytes = std::fs::read(&path).unwrap();
        assert!(bytes.len() > 8192);
        // Overwrite everything after the first page with junk; the header stays valid.
        let page: usize = u16::from_be_bytes([bytes[16], bytes[17]]) as usize;
        for b in bytes.iter_mut().skip(page) {
            *b = 0x5A;
        }
        std::fs::write(&path, &bytes).unwrap();
        assert!(matches!(check_file(&path), FileCheck::Corrupt(_)));
    }
}
