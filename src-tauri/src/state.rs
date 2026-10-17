use crate::capture::RecorderStatus;
use crate::config::{Paths, Settings};
use crate::db::Db;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, RwLock};

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisStatus {
    pub current_batch: Option<i64>,
    pub current_step: Option<String>,
    pub last_error: Option<String>,
    pub last_success_at: Option<i64>,
}

pub struct AppState {
    pub db: Db,
    pub paths: Paths,
    pub settings: RwLock<Settings>,
    pub recorder: Mutex<RecorderStatus>,
    pub analysis: Mutex<AnalysisStatus>,
    pub capture_now: AtomicBool,
    pub analyze_now: tokio::sync::Notify,
    /// Goes up by one with every saved settings change (the settings form, the tray, the bridge,
    /// the platform loop), always while the settings' write lock is held. `get_settings` hands it
    /// out and `save_settings` refuses a save made from an older one, so a change made in between
    /// is never overwritten unseen.
    pub settings_revision: AtomicU64,
}

impl AppState {
    pub fn new(paths: Paths) -> anyhow::Result<AppState> {
        crate::features::storage::backup::check_database(&paths.root, &paths.db);
        let db = Db::open(&paths.db)?;
        let settings = Settings::load(&paths.settings);
        Ok(AppState {
            db,
            paths,
            settings: RwLock::new(settings),
            recorder: Mutex::new(RecorderStatus::Off),
            analysis: Mutex::new(AnalysisStatus::default()),
            capture_now: AtomicBool::new(false),
            analyze_now: tokio::sync::Notify::new(),
            settings_revision: AtomicU64::new(1),
        })
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }

    pub fn settings_revision(&self) -> u64 {
        self.settings_revision.load(Ordering::SeqCst)
    }

    /// The settings and the revision they belong to. Writers count up while they hold the write
    /// lock, so a copy taken between two equal readings of the counter is that revision's.
    pub fn settings_with_revision(&self) -> (Settings, u64) {
        loop {
            let before = self.settings_revision();
            let s = self.settings();
            if self.settings_revision() == before {
                return (s, before);
            }
        }
    }

    /// Puts saved settings in memory and counts the change. Callers hold `config::save_lock()`
    /// and have written the file. Returns the new revision.
    pub fn replace_settings(&self, s: Settings) -> u64 {
        let mut g = self.settings.write().unwrap();
        *g = s;
        self.settings_revision.fetch_add(1, Ordering::SeqCst) + 1
    }

    /// Changes the settings in memory in place and counts the change (`replace_settings`).
    pub fn change_settings_in_memory(&self, f: impl FnOnce(&mut Settings)) -> Settings {
        let mut g = self.settings.write().unwrap();
        f(&mut g);
        self.settings_revision.fetch_add(1, Ordering::SeqCst);
        g.clone()
    }
}
