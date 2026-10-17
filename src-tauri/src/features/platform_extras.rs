//! Platform features for Dayflow parity: first-run setup, timed pause, the tray's state, day
//! rollover support, Windows notifications, the update check, the private-app picker and the
//! light/dark override. A loop (`spawn`) resumes timed pauses, keeps the tray in sync and sends
//! notifications; the pure logic lives in the submodules.

pub mod apps;
pub mod notify;
pub mod pause;
pub mod tray_state;
pub mod update;

pub use notify::NotificationPrefs;

use crate::capture::RecorderStatus;
use crate::config::{ProviderKind, Settings};
use crate::state::AppState;
use crate::timeutil::{logical_day, now_ts};
use anyhow::Result;
use notify::{Kind, RateLimiter, Target};
use pause::PauseChoice;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use update::UpdateStatus;

/// Tables owned by this feature (CREATE TABLE IF NOT EXISTS ...).
pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS platform_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;

/// Light/dark override (Dayflow: Settings → Other → Appearance).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Appearance {
    #[default]
    System,
    Light,
    Dark,
}

pub fn window_theme(a: Appearance) -> Option<tauri::Theme> {
    match a {
        Appearance::System => None,
        Appearance::Light => Some(tauri::Theme::Light),
        Appearance::Dark => Some(tauri::Theme::Dark),
    }
}

// ---- small key-value state ----

const K_RATE: &str = "notify_rate";
const K_RECAP_SEEN: &str = "recap_seen";
const K_FIRST_CARDS: &str = "first_cards";
const K_UPDATE: &str = "update_status";
const K_UPDATE_NOTIFIED: &str = "update_notified";

pub fn kv_get(state: &AppState, key: &str) -> Option<String> {
    let c = state.db.conn();
    c.query_row("SELECT value FROM platform_state WHERE key = ?", [key], |r| r.get(0)).optional().ok().flatten()
}

pub fn kv_set(state: &AppState, key: &str, value: &str) {
    let c = state.db.conn();
    if let Err(e) = c.execute(
        "INSERT INTO platform_state (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    ) {
        log::warn!("platform_state {key}: {e}");
    }
}

/// Changes the settings in memory and on disk.
pub fn update_settings(state: &AppState, f: impl FnOnce(&mut Settings)) -> Result<Settings> {
    let s = {
        let mut g = state.settings.write().unwrap();
        f(&mut g);
        g.clone()
    };
    s.save(&state.paths.settings)?;
    Ok(s)
}

// ---- first run ----

/// True for a settings file written before onboarding existed (no `onboardingCompleted` key):
/// that install is already set up and keeps recording. A file that does not parse also counts,
/// so a damaged file never stops an existing install from recording.
pub fn is_existing_install(raw_settings: Option<&str>) -> bool {
    match raw_settings {
        None => false,
        Some(raw) => match serde_json::from_str::<serde_json::Value>(raw) {
            Ok(serde_json::Value::Object(m)) => !m.contains_key("onboardingCompleted"),
            _ => true,
        },
    }
}

/// Runs before the recorder starts.
pub fn init(state: &AppState) {
    let raw = std::fs::read_to_string(&state.paths.settings).ok();
    if is_existing_install(raw.as_deref()) {
        log::info!("existing install: marking first-run setup as done");
        if let Err(e) = update_settings(state, |s| s.onboarding_completed = true) {
            log::error!("could not save settings: {e:#}");
        }
        kv_set(state, K_FIRST_CARDS, "done");
    }
}

/// Ends the first-run setup. `recording` turns recording on or off; on the first run Daytrace
/// will say when the first cards are ready.
pub fn finish_onboarding(state: &AppState, recording: bool) -> Result<Settings> {
    let first_run = !state.settings().onboarding_completed;
    let s = update_settings(state, |s| {
        s.onboarding_completed = true;
        s.recording_enabled = recording;
        s.pause_until = None;
    })?;
    if first_run && kv_get(state, K_FIRST_CARDS).is_none() {
        kv_set(state, K_FIRST_CARDS, &format!("pending:{}", now_ts()));
    }
    if recording {
        state.capture_now.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    Ok(s)
}

// ---- pause ----

fn after_change(app: &AppHandle) {
    let _ = app.emit("settings-changed", ());
    crate::tray::refresh(app);
}

pub fn pause_recording(app: &AppHandle, choice: PauseChoice) -> Result<()> {
    let state = app.state::<Arc<AppState>>();
    let s = update_settings(&state, |s| pause::apply_pause(s, choice, &chrono::Local::now()))?;
    log::info!("recording paused ({}), until {:?}", choice.id(), s.pause_until);
    after_change(app);
    Ok(())
}

pub fn resume_recording(app: &AppHandle) -> Result<()> {
    let state = app.state::<Arc<AppState>>();
    update_settings(&state, pause::apply_resume)?;
    state.capture_now.store(true, std::sync::atomic::Ordering::SeqCst);
    log::info!("recording resumed");
    after_change(app);
    Ok(())
}

/// Resumes an expired timed pause or clears a stale pause end. Returns whether settings changed.
pub fn apply_due_pause(state: &AppState, now: i64) -> Result<bool> {
    match pause::due(&state.settings(), now) {
        pause::Due::Nothing => Ok(false),
        pause::Due::Resume => {
            update_settings(state, |s| {
                // Re-check under the lock: the user may have changed it meanwhile.
                if pause::due(s, now) == pause::Due::Resume {
                    pause::apply_resume(s);
                }
            })?;
            state.capture_now.store(true, std::sync::atomic::Ordering::SeqCst);
            log::info!("timed pause ended; recording resumed");
            Ok(true)
        }
        pause::Due::ClearStale => {
            update_settings(state, |s| {
                if s.recording_enabled {
                    s.pause_until = None;
                }
            })?;
            Ok(true)
        }
    }
}

// ---- notifications ----

/// What the loop remembers between checks.
#[derive(Debug, Default)]
pub struct Watch {
    capture_error_since: Option<i64>,
}

/// Capture errors must last this long before a notification.
const CAPTURE_ERROR_GRACE: i64 = 120;
/// Analysis failures older than this are not reported (e.g. at startup after a night off).
const ANALYSIS_LOOKBACK: i64 = 3 * 3600;
/// A recap made longer ago than this is not announced anymore.
const RECAP_FRESH: i64 = 3600;

/// The notifications the current state calls for, with the detail text for each.
pub fn detect(state: &AppState, watch: &mut Watch, now: i64) -> Vec<(Kind, String)> {
    let settings = state.settings();
    let mut out = Vec::new();
    if !settings.onboarding_completed {
        return out;
    }

    // Recording problems: the recorder has reported an error for a while.
    let recorder = state.recorder.lock().unwrap().clone();
    match (&recorder, pause::mode(&settings)) {
        (RecorderStatus::Error(detail), pause::Mode::Active) => {
            let since = *watch.capture_error_since.get_or_insert(now);
            if now - since >= CAPTURE_ERROR_GRACE {
                out.push((Kind::RecordingProblem, detail.clone()));
            }
        }
        _ => watch.capture_error_since = None,
    }

    // Analysis problems, newest batches first.
    if let Ok(outcomes) = recent_outcomes(state, now - ANALYSIS_LOOKBACK) {
        if let Some((kind, streak)) = notify::analysis_kind(&outcomes, settings.provider) {
            out.push((kind, streak.to_string()));
        }
    }

    // Screenshots wait for an API key (the analysis loop keeps such batches pending).
    if settings.provider == ProviderKind::Anthropic
        && state.db.count_batches_with_status(&["pending"]).unwrap_or(0) > 0
        && crate::config::stored_key(settings.provider, &settings.base_url).is_none()
    {
        out.push((Kind::MissingKey, String::new()));
    }

    // First cards after setup (once).
    if let Some(since) = kv_get(state, K_FIRST_CARDS).and_then(|v| v.strip_prefix("pending:").and_then(|t| t.parse::<i64>().ok())) {
        let real = state
            .db
            .cards_in_range(since - 3600, now + 60, true)
            .map(|cards| cards.iter().any(|c| !c.is_idle_shortcut && !c.title.eq_ignore_ascii_case("idle")))
            .unwrap_or(false);
        if real {
            kv_set(state, K_FIRST_CARDS, "done");
            out.push((Kind::FirstCards, String::new()));
        }
    }

    // Daily recap written (by the insights scheduler) for today's target day.
    let target = logical_day(now);
    if kv_get(state, K_RECAP_SEEN).as_deref() != Some(target.as_str()) {
        if let Some(created) = recap_created_at(state, &target) {
            kv_set(state, K_RECAP_SEEN, &target);
            if now - created <= RECAP_FRESH {
                out.push((Kind::RecapReady, target));
            }
        }
    }
    out
}

/// `(failed, reason)` of analysed batches since `since`, newest first.
fn recent_outcomes(state: &AppState, since: i64) -> Result<Vec<(bool, Option<String>)>> {
    let c = state.db.conn();
    let mut st = c.prepare(
        "SELECT status, reason FROM analysis_batches
         WHERE batch_end_ts >= ? AND status IN ('completed', 'analyzed', 'failed')
         ORDER BY batch_end_ts DESC, id DESC LIMIT 10",
    )?;
    let rows = st.query_map([since], |r| Ok((r.get::<_, String>(0)? == "failed", r.get::<_, Option<String>>(1)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// When the recap for `target` was created, if there is one. Reads the insights feature's table
/// (read-only); returns None when that table does not exist.
fn recap_created_at(state: &AppState, target: &str) -> Option<i64> {
    let c = state.db.conn();
    c.query_row("SELECT created_at FROM daily_recaps WHERE target_day = ?", [target], |r| r.get::<_, i64>(0)).optional().ok().flatten()
}

/// Whether a notification of this kind may be shown now; records it when it may.
/// `focused`: the main window is in front, so the user sees the problem in the app already.
pub fn admit(state: &AppState, kind: Kind, detail: &str, now: i64, focused: bool) -> bool {
    let settings = state.settings();
    if !kind.enabled(&settings.notifications) {
        return false;
    }
    if kind == Kind::Test {
        return true;
    }
    if focused {
        return false;
    }
    if kind == Kind::Update && kv_get(state, K_UPDATE_NOTIFIED).as_deref() == Some(detail) {
        return false;
    }
    let mut rl: RateLimiter = kv_get(state, K_RATE).and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default();
    let day = if kind == Kind::RecapReady { detail.to_string() } else { logical_day(now) };
    if !rl.allow(&kind.key(), &day, now) {
        return false;
    }
    kv_set(state, K_RATE, &serde_json::to_string(&rl).unwrap_or_default());
    if kind == Kind::Update {
        kv_set(state, K_UPDATE_NOTIFIED, detail);
    }
    true
}

fn main_window_focused(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .map(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false) && !w.is_minimized().unwrap_or(false))
        .unwrap_or(false)
}

fn show_toast(app: &AppHandle, id: i32, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    if let Err(e) = app.notification().builder().id(id).title(title).body(body).show() {
        log::warn!("notification failed: {e}");
    }
}

/// Shows a notification when the settings and the rate limit allow it.
pub fn notify(app: &AppHandle, kind: Kind, detail: &str) -> bool {
    let state = app.state::<Arc<AppState>>();
    if !admit(&state, kind, detail, now_ts(), main_window_focused(app)) {
        return false;
    }
    let (title, body) = notify::text(kind, state.settings().provider, detail);
    log::info!("notification: {title}");
    show_toast(app, kind.toast_id(), &title, &body);
    true
}

// ---- navigation from notifications ----

static PENDING_NAV: Mutex<Option<Target>> = Mutex::new(None);

fn navigate(app: &AppHandle, target: Target) {
    if target == Target::Release {
        let _ = open_release(app);
        return;
    }
    *PENDING_NAV.lock().unwrap() = Some(target);
    crate::tray::show_main(app);
    let _ = app.emit("platform-navigate", target);
}

// ---- update check ----

static CHECKING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn update_status(state: &AppState) -> UpdateStatus {
    let mut st: UpdateStatus = kv_get(state, K_UPDATE).and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default();
    let current = env!("CARGO_PKG_VERSION");
    if st.current_version != current {
        // Updated since the last check: "available" referred to the old version.
        st.available = st.latest_version.as_deref().and_then(update::Version::parse).zip(update::Version::parse(current)).is_some_and(|(l, c)| l > c);
        st.current_version = current.to_string();
    }
    st
}

/// Checks GitHub now and stores the result. `announce`: show a notification with the outcome
/// (the tray's "Check for updates").
pub async fn check_updates(app: &AppHandle, announce: bool) -> UpdateStatus {
    let state = app.state::<Arc<AppState>>().inner().clone();
    if CHECKING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return update_status(&state);
    }
    let current = env!("CARGO_PKG_VERSION");
    let st = update::check(current, now_ts(), || update::fetch_github(current)).await;
    CHECKING.store(false, std::sync::atomic::Ordering::SeqCst);
    kv_set(&state, K_UPDATE, &serde_json::to_string(&st).unwrap_or_default());
    log::info!("update check: latest {:?}, available {}, error {:?}", st.latest_version, st.available, st.error);
    let _ = app.emit("platform-update", &st);
    crate::tray::refresh(app);
    if announce {
        let (title, body) = match (&st.error, st.available, &st.latest_version) {
            (Some(e), _, _) => ("Couldn’t check for updates".to_string(), e.clone()),
            (None, true, Some(v)) => (format!("Daytrace {v} is available"), "Click to open the release page.".to_string()),
            (None, _, _) if st.no_releases => ("No releases yet".to_string(), "No Daytrace release has been published on GitHub yet.".to_string()),
            _ => ("Daytrace is up to date".to_string(), format!("You have the latest version ({current}).")),
        };
        show_toast(app, if st.available { Kind::Update.toast_id() } else { Kind::Test.toast_id() }, &title, &body);
    } else if st.available {
        if let Some(v) = &st.latest_version {
            notify(app, Kind::Update, v);
        }
    }
    st
}

fn open_release(app: &AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let state = app.state::<Arc<AppState>>();
    let url = update_status(&state).release_url.filter(|u| update::is_release_url(u)).unwrap_or_else(|| format!("https://github.com/{}/releases", update::REPO));
    app.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
}

// ---- the loop ----

/// Starts the platform loop: once a second it resumes expired pauses and syncs the tray; every
/// 30 seconds it looks for notifications; once a day it checks for updates.
pub fn spawn(app: AppHandle) {
    {
        use tauri_plugin_notification::NotificationExt;
        let a = app.clone();
        let _ = app.notification().on_action(move |action| {
            if action.action_id() == "tap" {
                let target = action.notification().map(|n| Kind::target_for_toast_id(n.id())).unwrap_or(Target::Timeline);
                navigate(&a, target);
            }
        });
    }
    std::thread::Builder::new()
        .name("platform".into())
        .spawn(move || {
            let state = app.state::<Arc<AppState>>().inner().clone();
            let started = now_ts();
            let mut watch = Watch::default();
            let mut last_notify_check = 0i64;
            let mut last_update_check = 0i64;
            loop {
                let now = now_ts();
                match apply_due_pause(&state, now) {
                    Ok(true) => {
                        let _ = app.emit("settings-changed", ());
                        crate::tray::refresh(&app);
                    }
                    Ok(false) => {}
                    Err(e) => log::error!("pause: {e:#}"),
                }
                crate::tray::sync(&app);

                if now - last_notify_check >= 30 {
                    last_notify_check = now;
                    for (kind, detail) in detect(&state, &mut watch, now) {
                        notify(&app, kind, &detail);
                    }
                }
                // First automatic update check two minutes after start, then once a day.
                if now - started >= 120 && now - last_update_check >= 600 {
                    last_update_check = now;
                    if state.settings().check_updates && update::due(&update_status(&state), now) {
                        let a = app.clone();
                        tauri::async_runtime::spawn(async move {
                            check_updates(&a, false).await;
                        });
                    }
                }
                std::thread::sleep(Duration::from_secs(1));
            }
        })
        .expect("spawn platform loop");
}

// ---- commands ----

type S<'a> = tauri::State<'a, Arc<AppState>>;
type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PauseOption {
    id: String,
    label: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformInfo {
    version: String,
    pause_options: Vec<PauseOption>,
    minute_options: Vec<u32>,
    update: UpdateStatus,
}

#[tauri::command]
pub fn platform_info(state: S) -> PlatformInfo {
    let settings = state.settings();
    PlatformInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        pause_options: pause::choices(&settings).into_iter().map(|c| PauseOption { id: c.id(), label: c.label() }).collect(),
        minute_options: pause::MINUTE_OPTIONS.to_vec(),
        update: update_status(&state),
    }
}

#[tauri::command]
pub fn platform_pause(app: AppHandle, choice: String) -> CmdResult<()> {
    let c = PauseChoice::parse(&choice).ok_or_else(|| format!("Unknown pause length: {choice}"))?;
    pause_recording(&app, c).map_err(err)
}

#[tauri::command]
pub fn platform_resume(app: AppHandle) -> CmdResult<()> {
    resume_recording(&app).map_err(err)
}

#[tauri::command]
pub fn platform_finish_onboarding(app: AppHandle, state: S, recording: bool) -> CmdResult<Settings> {
    let s = finish_onboarding(&state, recording).map_err(err)?;
    after_change(&app);
    Ok(s)
}

#[tauri::command]
pub fn platform_take_navigation() -> Option<Target> {
    PENDING_NAV.lock().unwrap().take()
}

#[tauri::command]
pub fn platform_set_window_theme(app: AppHandle, appearance: Appearance) -> CmdResult<()> {
    if let Some(w) = app.get_webview_window("main") {
        w.set_theme(window_theme(appearance)).map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn platform_app_candidates(state: S<'_>) -> CmdResult<Vec<apps::AppCandidate>> {
    let history = apps::history(&state.db, now_ts() - apps::HISTORY_DAYS * 86400).map_err(err)?;
    let running = tauri::async_runtime::spawn_blocking(apps::running_apps).await.map_err(err)?;
    Ok(apps::merge(&history, &running, &state.settings().blocked_apps))
}

#[tauri::command]
pub async fn platform_check_updates(app: AppHandle) -> UpdateStatus {
    check_updates(&app, false).await
}

#[tauri::command]
pub fn platform_open_release(app: AppHandle) -> CmdResult<()> {
    open_release(&app)
}

#[tauri::command]
pub fn platform_open_screenshots(app: AppHandle, state: S) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_path(state.paths.screenshots.to_string_lossy(), None::<&str>).map_err(err)
}

#[tauri::command]
pub fn platform_test_notification(app: AppHandle) {
    notify(&app, Kind::Test, "");
}

/// Opens the screenshots folder (tray menu).
pub fn open_screenshots(app: &AppHandle) {
    use tauri_plugin_opener::OpenerExt;
    let state = app.state::<Arc<AppState>>();
    if let Err(e) = app.opener().open_path(state.paths.screenshots.to_string_lossy(), None::<&str>) {
        log::warn!("could not open the screenshots folder: {e}");
    }
}

/// Opens the release page (tray menu).
pub fn open_release_page(app: &AppHandle) {
    if let Err(e) = open_release(app) {
        log::warn!("could not open the release page: {e}");
    }
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

    #[test]
    fn detects_existing_installs() {
        assert!(!is_existing_install(None), "fresh install");
        assert!(is_existing_install(Some(r#"{"recordingEnabled":true}"#)));
        assert!(!is_existing_install(Some(r#"{"recordingEnabled":true,"onboardingCompleted":false}"#)));
        assert!(is_existing_install(Some("{broken")));
    }

    #[test]
    fn onboarding_flag_persists() {
        let (dir, st) = state();
        // Fresh install: no settings file, setup pending, recorder held.
        init(&st);
        assert!(!st.settings().onboarding_completed);
        assert!(!pause::recording_allowed(&st.settings()));
        let s = finish_onboarding(&st, true).unwrap();
        assert!(s.onboarding_completed && s.recording_enabled);
        assert!(kv_get(&st, K_FIRST_CARDS).unwrap().starts_with("pending:"));
        // Reloaded from disk after a restart.
        let st2 = AppState::new(Paths::new(dir.path().to_path_buf()).unwrap()).unwrap();
        assert!(st2.settings().onboarding_completed);
        init(&st2);
        assert!(pause::recording_allowed(&st2.settings()));
    }

    #[test]
    fn upgrade_marks_setup_done() {
        let (dir, st) = state();
        std::fs::write(dir.path().join("settings.json"), r#"{"recordingEnabled":true,"retentionDays":3}"#).unwrap();
        let st = { drop(st); AppState::new(Paths::new(dir.path().to_path_buf()).unwrap()).unwrap() };
        init(&st);
        assert!(st.settings().onboarding_completed);
        assert_eq!(st.settings().retention_days, 3);
        assert_eq!(kv_get(&st, K_FIRST_CARDS).as_deref(), Some("done"));
        let raw = std::fs::read_to_string(dir.path().join("settings.json")).unwrap();
        assert!(raw.contains("\"onboardingCompleted\": true"));
    }

    #[test]
    fn finishing_without_recording() {
        let (_d, st) = state();
        let s = finish_onboarding(&st, false).unwrap();
        assert!(s.onboarding_completed && !s.recording_enabled);
        assert_eq!(pause::mode(&s), pause::Mode::PausedIndefinite);
        // Running setup again later does not ask for first cards again.
        kv_set(&st, K_FIRST_CARDS, "done");
        finish_onboarding(&st, true).unwrap();
        assert_eq!(kv_get(&st, K_FIRST_CARDS).as_deref(), Some("done"));
    }

    #[test]
    fn resumes_expired_pause() {
        let (_d, st) = state();
        update_settings(&st, |s| {
            s.onboarding_completed = true;
            s.recording_enabled = false;
            s.pause_until = Some(1000);
        })
        .unwrap();
        assert!(!apply_due_pause(&st, 999).unwrap());
        assert!(!st.settings().recording_enabled);
        assert!(apply_due_pause(&st, 1000).unwrap());
        let s = st.settings();
        assert!(s.recording_enabled && s.pause_until.is_none());
        let on_disk = Settings::load(&st.paths.settings);
        assert!(on_disk.recording_enabled && on_disk.pause_until.is_none());
    }

    fn batch(st: &AppState, end: i64, status: &str, reason: Option<&str>) {
        st.db.insert_screenshot(end - 900, "", 0, 0, None, None, None, None).unwrap();
        let shots = st.db.unbatched_screenshots(0).unwrap();
        let id = st.db.create_batch(&shots).unwrap();
        st.db.conn().execute("UPDATE analysis_batches SET batch_end_ts = ? WHERE id = ?", rusqlite::params![end, id]).unwrap();
        st.db.set_batch_status(id, status, reason).unwrap();
    }

    #[test]
    fn detects_notifications() {
        let (_d, st) = state();
        let now = now_ts();
        update_settings(&st, |s| {
            s.onboarding_completed = true;
            s.provider = ProviderKind::Ollama;
        })
        .unwrap();
        let mut w = Watch::default();
        assert!(detect(&st, &mut w, now).is_empty());

        // Capture errors count after the grace period only.
        *st.recorder.lock().unwrap() = RecorderStatus::Error("no monitors found".into());
        assert!(detect(&st, &mut w, now).is_empty());
        let found = detect(&st, &mut w, now + CAPTURE_ERROR_GRACE);
        assert_eq!(found, vec![(Kind::RecordingProblem, "no monitors found".to_string())]);
        *st.recorder.lock().unwrap() = RecorderStatus::Recording;
        assert!(detect(&st, &mut w, now).is_empty());

        // The local model is offline.
        batch(&st, now - 1800, "completed", None);
        batch(&st, now - 900, "failed", Some("Could not reach the model server: error sending request"));
        let found = detect(&st, &mut w, now);
        assert_eq!(found, vec![(Kind::Analysis(notify::Failure::LocalOffline), "1".to_string())]);
    }

    #[test]
    fn detects_recap_once() {
        let (_d, st) = state();
        let now = now_ts();
        update_settings(&st, |s| s.onboarding_completed = true).unwrap();
        let target = logical_day(now);
        st.db
            .conn()
            .execute("INSERT INTO daily_recaps (target_day, source_day, created_at, updated_at) VALUES (?1, ?1, ?2, ?2)", rusqlite::params![target, now - 60])
            .unwrap();
        let mut w = Watch::default();
        assert_eq!(detect(&st, &mut w, now), vec![(Kind::RecapReady, target.clone())]);
        assert!(detect(&st, &mut w, now + 30).is_empty(), "announced once");
    }

    #[test]
    fn detects_first_cards() {
        let (_d, st) = state();
        let now = now_ts();
        finish_onboarding(&st, true).unwrap();
        let mut w = Watch::default();
        assert!(detect(&st, &mut w, now).is_empty());
        let card = crate::types::ActivityCard {
            start_time: crate::timeutil::fmt_clock(now - 600),
            end_time: crate::timeutil::fmt_clock(now),
            category: "Work".into(),
            subcategory: String::new(),
            title: "Writing the report".into(),
            summary: String::new(),
            detailed_summary: String::new(),
            distractions: None,
            app_sites: None,
        };
        st.db.insert_screenshot(now - 600, "", 0, 0, None, None, None, None).unwrap();
        let b = st.db.create_batch(&st.db.unbatched_screenshots(0).unwrap()).unwrap();
        st.db.set_batch_status(b, "completed", None).unwrap();
        let rc = crate::db::ResolvedCard { start_ts: now - 600, end_ts: now, card, idle_meta: None };
        st.db.replace_cards_in_range(now - 600, now, &[rc], b, true).unwrap();
        assert_eq!(detect(&st, &mut w, now), vec![(Kind::FirstCards, String::new())]);
        assert!(detect(&st, &mut w, now).is_empty());
    }

    #[test]
    fn admits_within_limits() {
        let (_d, st) = state();
        let now = 1_800_000_000;
        assert!(admit(&st, Kind::MissingKey, "", now, false));
        assert!(!admit(&st, Kind::MissingKey, "", now + 3600, false), "once a day");
        assert!(!admit(&st, Kind::RecordingProblem, "", now + 10, false), "minimum gap");
        assert!(!admit(&st, Kind::RecordingProblem, "", now + 3600, true), "window in front");
        assert!(admit(&st, Kind::RecordingProblem, "", now + 3600, false));
        update_settings(&st, |s| s.notifications.updates = false).unwrap();
        assert!(!admit(&st, Kind::Update, "0.3.0", now + 7200, false), "turned off");
        update_settings(&st, |s| s.notifications.updates = true).unwrap();
        assert!(admit(&st, Kind::Update, "0.3.0", now + 7200, false));
        assert!(!admit(&st, Kind::Update, "0.3.0", now + 3 * 86400, false), "once per version");
        assert!(admit(&st, Kind::Test, "", now + 7201, true), "test always shows");
    }

    #[test]
    fn update_status_follows_running_version() {
        let (_d, st) = state();
        let stored = UpdateStatus { current_version: "0.0.1".into(), latest_version: Some("0.0.2".into()), available: true, ..Default::default() };
        kv_set(&st, K_UPDATE, &serde_json::to_string(&stored).unwrap());
        let now = update_status(&st);
        assert_eq!(now.current_version, env!("CARGO_PKG_VERSION"));
        assert!(!now.available, "already running something newer than 0.0.2");
    }
}
