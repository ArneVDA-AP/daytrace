use crate::capture::RecorderStatus;
use crate::config::{Paths, Settings};
use crate::db::Db;
use serde::Serialize;
use std::sync::atomic::AtomicBool;
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
        })
    }

    pub fn settings(&self) -> Settings {
        self.settings.read().unwrap().clone()
    }
}
