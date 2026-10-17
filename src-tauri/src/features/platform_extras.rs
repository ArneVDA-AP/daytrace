//! Platform features for Dayflow parity: first-run setup, timed pause, pause and resume from
//! outside the window, the tray's state, day rollover support, Windows notifications, the update
//! check and the installing of updates, "What's new", "Copy debug info", the private-app picker
//! and the light/dark override. A loop (`spawn`) resumes timed pauses, keeps the tray in sync,
//! sends notifications and installs downloaded updates; the pure logic lives in the submodules.

pub mod apps;
pub mod autoupdate;
pub mod debug_info;
pub mod notify;
pub mod pause;
pub mod switches;
pub mod tray_state;
pub mod update;
pub mod whats_new;

pub use notify::NotificationPrefs;

use crate::capture::RecorderStatus;
use crate::config::Settings;
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
/// The version whose "What's new" was shown (a fresh install records its own version).
const K_WHATS_NEW: &str = "whats_new_seen";

pub fn kv_get(state: &AppState, key: &str) -> Option<String> {
    let c = state.db.conn();
    c.query_row("SELECT value FROM platform_state WHERE key = ?", [key], |r| r.get(0)).optional().ok().flatten()
}

pub fn kv_delete(state: &AppState, key: &str) {
    let _ = state.db.conn().execute("DELETE FROM platform_state WHERE key = ?", [key]);
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

/// Changes the settings in memory and on disk. The save lock is held from the change through the
/// save, so concurrent changes (the settings form, the tray, the platform loop) are written in
/// the order they were made and the file always matches memory.
pub fn update_settings(state: &AppState, f: impl FnOnce(&mut Settings)) -> Result<Settings> {
    let _saving = crate::config::save_lock();
    let s = {
        let mut g = state.settings.write().unwrap();
        f(&mut g);
        g.clone()
    };
    s.save(&state.paths.settings)?;
    Ok(s)
}

// ---- first run ----

/// What the startup finds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Install {
    /// No settings and no recordings yet: the first-run setup comes up.
    Fresh,
    /// Set up before onboarding existed: a settings file without `onboardingCompleted`, or no
    /// settings file at all but screenshot rows in the database (an install that never saved
    /// its settings). It keeps recording.
    Existing,
    /// A settings file of this version.
    Current,
    /// A settings file that does not parse (a typo in a hand edit, a value a newer version
    /// wrote). It is kept as `settings.json.bad` and recording waits in the setup.
    Damaged,
}

pub fn classify_install(raw_settings: Option<&str>, has_recordings: bool) -> Install {
    let Some(raw) = raw_settings else {
        return if has_recordings { Install::Existing } else { Install::Fresh };
    };
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(serde_json::Value::Object(m)) if serde_json::from_str::<Settings>(raw).is_ok() => {
            if m.contains_key("onboardingCompleted") { Install::Current } else { Install::Existing }
        }
        _ => Install::Damaged,
    }
}

/// Where a settings file that did not parse is kept.
pub fn bad_settings_path(state: &AppState) -> std::path::PathBuf {
    state.paths.settings.with_extension("json.bad")
}

const K_SETTINGS_DAMAGED: &str = "settings_damaged";

/// Runs before the recorder starts.
pub fn init(state: &AppState) {
    let raw = std::fs::read_to_string(&state.paths.settings).ok();
    let has_recordings = state.db.conn().query_row("SELECT EXISTS(SELECT 1 FROM screenshots)", [], |r| r.get::<_, bool>(0)).unwrap_or(false);
    match classify_install(raw.as_deref(), has_recordings) {
        Install::Fresh => kv_set(state, K_WHATS_NEW, env!("CARGO_PKG_VERSION")),
        Install::Current => {}
        Install::Existing => {
            log::info!("existing install: marking first-run setup as done");
            if let Err(e) = update_settings(state, |s| s.onboarding_completed = true) {
                log::error!("could not save settings: {e:#}");
            }
            kv_set(state, K_FIRST_CARDS, "done");
        }
        Install::Damaged => {
            // Nothing is saved over the file here. The defaults in memory have the setup not
            // done, so recording waits until the user goes through the setup again.
            let bad = bad_settings_path(state);
            match std::fs::copy(&state.paths.settings, &bad) {
                Ok(_) => log::error!("settings.json could not be read; kept as {} and opened the setup", bad.display()),
                Err(e) => log::error!("settings.json could not be read, and copying it to {} failed: {e}", bad.display()),
            }
            {
                let mut g = state.settings.write().unwrap();
                g.onboarding_completed = false;
            }
            kv_set(state, K_SETTINGS_DAMAGED, &bad.display().to_string());
        }
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
    kv_delete(state, K_SETTINGS_DAMAGED);
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
    if let Ok(outcomes) = recent_outcomes(state, now - ANALYSIS_LOOKBACK)
        && let Some((kind, streak)) = notify::analysis_kind(&outcomes, settings.provider)
    {
        out.push((kind, streak.to_string()));
    }

    // Screenshots wait for an API key (the analysis loop keeps such batches pending; a usable
    // fallback provider analyzes them instead).
    if crate::llm::routing::waits_for_key(&settings) && state.db.count_batches_with_status(&["pending"]).unwrap_or(0) > 0 {
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
            out.push((Kind::FirstCards, String::new()));
        }
    }

    // A newer release (found by the daily check), once per version.
    let up = update_status(state);
    if let (true, Some(v)) = (up.available, up.latest_version)
        && kv_get(state, K_UPDATE_NOTIFIED).as_deref() != Some(v.as_str())
        && !(settings.auto_install_updates && autoupdate::installs_itself(&install_status(), &v))
    {
        out.push((Kind::Update, v));
    }

    // Daily recap written (by the insights scheduler) for today's target day.
    let target = logical_day(now);
    if kv_get(state, K_RECAP_SEEN).as_deref() != Some(target.as_str())
        && let Some(created) = recap_created_at(state, &target)
    {
        if now - created <= RECAP_FRESH {
            out.push((Kind::RecapReady, target));
        } else {
            kv_set(state, K_RECAP_SEEN, &target);
        }
    }
    out
}

/// Records that a one-time notification was dealt with (shown, or skipped because it is turned
/// off or the window was in front), so it is not offered again. A notification held back by the
/// minimum gap stays open and is offered on the next check.
pub fn settle(state: &AppState, kind: Kind, detail: &str, outcome: Outcome) {
    if outcome == Outcome::Later {
        return;
    }
    match kind {
        Kind::RecapReady => kv_set(state, K_RECAP_SEEN, detail),
        Kind::FirstCards => kv_set(state, K_FIRST_CARDS, "done"),
        _ => {}
    }
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

/// What happened to a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Shown,
    /// Turned off, already shown today, or the window is in front.
    Skipped,
    /// Held back by the minimum gap between notifications.
    Later,
}

/// Whether a notification of this kind may be shown now; records it when it may.
/// `focused`: the main window is in front, so the user sees the problem in the app already.
pub fn admit(state: &AppState, kind: Kind, detail: &str, now: i64, focused: bool) -> Outcome {
    let settings = state.settings();
    if !kind.enabled(&settings.notifications) {
        return Outcome::Skipped;
    }
    if kind == Kind::Test {
        return Outcome::Shown;
    }
    if focused {
        return Outcome::Skipped;
    }
    if kind == Kind::Update && kv_get(state, K_UPDATE_NOTIFIED).as_deref() == Some(detail) {
        return Outcome::Skipped;
    }
    let mut rl: RateLimiter = kv_get(state, K_RATE).and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default();
    let day = if kind == Kind::RecapReady { detail.to_string() } else { logical_day(now) };
    match rl.record(&kind.key(), &day, now) {
        notify::Decision::SameDay => return Outcome::Skipped,
        notify::Decision::TooSoon => return Outcome::Later,
        notify::Decision::Allow => {}
    }
    kv_set(state, K_RATE, &serde_json::to_string(&rl).unwrap_or_default());
    if kind == Kind::Update {
        kv_set(state, K_UPDATE_NOTIFIED, detail);
    }
    Outcome::Shown
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
pub fn notify(app: &AppHandle, kind: Kind, detail: &str) -> Outcome {
    let state = app.state::<Arc<AppState>>();
    let outcome = admit(&state, kind, detail, now_ts(), main_window_focused(app));
    if outcome == Outcome::Shown {
        let (title, body) = notify::text(kind, state.settings().provider, detail);
        log::info!("notification: {title}");
        show_toast(app, kind.toast_id(), &title, &body);
    }
    settle(&state, kind, detail, outcome);
    outcome
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
    let fresh = update::check(current, now_ts(), || update::fetch_github(current)).await;
    // A failed check keeps the release found before (and the tray item); it only adds the error.
    let st = update::merge(&update_status(&state), fresh);
    CHECKING.store(false, std::sync::atomic::Ordering::SeqCst);
    kv_set(&state, K_UPDATE, &serde_json::to_string(&st).unwrap_or_default());
    log::info!("update check: latest {:?}, available {}, error {:?}", st.latest_version, st.available, st.error);
    let _ = app.emit("platform-update", &st);
    crate::tray::set_update(st.latest_version.clone().filter(|_| st.available));
    crate::tray::refresh(app);
    if let (true, Some(v)) = (st.available, &st.latest_version)
        && state.settings().auto_install_updates
    {
        start_download(app, v);
    }
    if announce {
        let (title, body) = match (&st.error, st.available, &st.latest_version) {
            (Some(e), _, _) => ("Couldn’t check for updates".to_string(), e.clone()),
            (None, true, Some(v)) if state.settings().auto_install_updates => {
                (format!("Daytrace {v} is available"), "It installs when the PC is idle or locked. Click to open the release page.".to_string())
            }
            (None, true, Some(v)) => (format!("Daytrace {v} is available"), "Click to open the release page.".to_string()),
            (None, _, _) if st.no_releases => ("No releases yet".to_string(), "No Daytrace release has been published on GitHub yet.".to_string()),
            _ => ("Daytrace is up to date".to_string(), format!("You have the latest version ({current}).")),
        };
        show_toast(app, if st.available { Kind::Update.toast_id() } else { Kind::Test.toast_id() }, &title, &body);
    }
    st
}

// ---- installing updates (autoupdate.rs) ----

static INSTALL: Mutex<autoupdate::InstallStatus> = Mutex::new(autoupdate::InstallStatus::idle());
/// The downloaded, verified update, built to start Daytrace minimized after the install.
static PENDING: Mutex<Option<(tauri_plugin_updater::Update, Vec<u8>)>> = Mutex::new(None);

pub fn install_status() -> autoupdate::InstallStatus {
    INSTALL.lock().unwrap().clone()
}

fn set_install(app: &AppHandle, f: impl FnOnce(&mut autoupdate::InstallStatus)) {
    let st = {
        let mut g = INSTALL.lock().unwrap();
        f(&mut g);
        g.clone()
    };
    let _ = app.emit("platform-update-install", &st);
}

/// The folder of daytrace.exe and daytrace-mcp.exe.
fn exe_dir() -> Option<std::path::PathBuf> {
    std::env::current_exe().ok()?.parent().map(std::path::Path::to_path_buf)
}

/// The updater, set up to run the installer passively (tauri.conf.json) and start Daytrace again
/// (minimized for automatic installs). Before Daytrace quits for the installer, daytrace-mcp.exe
/// is moved aside so an MCP client that keeps it running does not block the install.
fn updater(app: &AppHandle, minimized: bool) -> Result<tauri_plugin_updater::Updater> {
    use tauri_plugin_updater::UpdaterExt;
    Ok(app
        .updater_builder()
        .timeout(Duration::from_secs(300))
        .restart_after_install(false)
        .installer_args(autoupdate::installer_args(minimized))
        .on_before_exit(|| {
            log::info!("starting the update installer; Daytrace quits now");
            if let Some(dir) = exe_dir() {
                match autoupdate::move_mcp_aside(&dir) {
                    Ok(true) => log::info!("moved {} aside for the installer", autoupdate::MCP_EXE),
                    Ok(false) => {}
                    Err(e) => log::warn!("could not move {} aside: {e}", autoupdate::MCP_EXE),
                }
            }
            log::logger().flush();
        })
        .build()?)
}

/// Downloads `latest` in the background when it is not downloaded or downloading already.
pub fn start_download(app: &AppHandle, latest: &str) {
    {
        let mut g = INSTALL.lock().unwrap();
        if !autoupdate::should_prepare(&g, latest, now_ts()) {
            return;
        }
        *g = autoupdate::InstallStatus { stage: autoupdate::Stage::Downloading, version: Some(latest.to_string()), ..autoupdate::InstallStatus::idle() };
    }
    let _ = app.emit("platform-update-install", install_status());
    let app = app.clone();
    let latest = latest.to_string();
    tauri::async_runtime::spawn(async move {
        match download(&app).await {
            // latest.json belongs to the same release as the check; a mismatch means the release
            // was changed meanwhile: no install, the next try is in six hours.
            Ok((update, _)) if !autoupdate::same_version(&update.version, &latest) => {
                let msg = format!("The signed update is version {}, not {latest}. Use the release page.", update.version);
                log::warn!("update: {msg}");
                set_install(&app, |s| {
                    s.stage = autoupdate::Stage::Failed;
                    s.error = Some(msg);
                    s.failed_at = Some(now_ts());
                });
            }
            Ok((update, bytes)) => {
                log::info!("update {} downloaded and verified ({} bytes); installs when the PC is idle or locked", update.version, bytes.len());
                *PENDING.lock().unwrap() = Some((update, bytes));
                set_install(&app, |s| {
                    s.stage = autoupdate::Stage::Ready;
                    s.error = None;
                });
            }
            Err(e) => {
                log::warn!("update {latest}: {e:#}");
                set_install(&app, |s| {
                    s.stage = autoupdate::Stage::Failed;
                    s.error = Some(format!("{e:#}"));
                    s.failed_at = Some(now_ts());
                });
            }
        }
    });
}

async fn download(app: &AppHandle) -> Result<(tauri_plugin_updater::Update, Vec<u8>)> {
    let update = updater(app, true)?
        .check()
        .await
        .map_err(|e| anyhow::anyhow!("Couldn’t read the signed update of this release ({e})"))?
        .ok_or_else(|| anyhow::anyhow!("This release has no signed update for Daytrace to install. Use the release page."))?;
    let a = app.clone();
    let mut got = 0u64;
    let mut shown = 0u64;
    let bytes = update
        .download(
            move |n, total| {
                got += n as u64;
                // About every megabyte, so the progress line moves without flooding the window.
                if got - shown >= 1 << 20 || total == Some(got) {
                    shown = got;
                    set_install(&a, |s| {
                        s.downloaded = got;
                        s.total = total;
                    });
                }
            },
            || {},
        )
        .await
        .map_err(|e| anyhow::anyhow!("The download failed or its signature did not match ({e})"))?;
    Ok((update, bytes))
}

/// Runs the installer of the downloaded update; on Windows Daytrace quits here.
fn run_installer(app: &AppHandle, update: &tauri_plugin_updater::Update, bytes: &[u8]) -> Result<()> {
    set_install(app, |s| s.stage = autoupdate::Stage::Installing);
    let result: Result<()> = if cfg!(windows) { update.install(bytes).map_err(anyhow::Error::from) } else { Err(anyhow::anyhow!("Updates install on Windows only.")) };
    if let Err(e) = &result {
        log::error!("update install failed: {e:#}");
        set_install(app, |s| {
            s.stage = autoupdate::Stage::Failed;
            s.error = Some(format!("Couldn’t start the installer: {e:#}"));
            s.failed_at = Some(now_ts());
        });
    }
    result
}

/// From the platform loop: installs the downloaded update when the PC is idle or locked.
fn maybe_install(app: &AppHandle, state: &AppState) {
    if INSTALL.lock().unwrap().stage != autoupdate::Stage::Ready || !state.settings().auto_install_updates {
        return;
    }
    let moment = autoupdate::Moment {
        idle_secs: crate::platform::idle_seconds(),
        locked: crate::platform::is_locked(),
        display_required: crate::platform::display_required(),
        analyzing: state.analysis.lock().unwrap().current_batch.is_some(),
    };
    if !autoupdate::may_install(&moment) {
        return;
    }
    let Some((update, bytes)) = PENDING.lock().unwrap().take() else { return };
    log::info!("installing update {} (idle {} s, locked {})", update.version, moment.idle_secs, moment.locked);
    let _ = run_installer(app, &update, &bytes);
}

/// "Install and restart now": a fresh check so the installer brings the window back, then the
/// downloaded bytes (verified again against the signature).
async fn install_now(app: &AppHandle) -> Result<()> {
    let (version, bytes) = match PENDING.lock().unwrap().as_ref() {
        Some((u, b)) => (u.version.clone(), b.clone()),
        None => anyhow::bail!("No update is downloaded yet."),
    };
    let update = updater(app, false)?.check().await?.ok_or_else(|| anyhow::anyhow!("The update is no longer offered."))?;
    if update.version != version {
        anyhow::bail!("A newer version ({}) was published meanwhile; it downloads on the next check.", update.version);
    }
    log::info!("installing update {version} now (asked in Settings)");
    run_installer(app, &update, &bytes)
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
            let mut last_install_check = 0i64;
            if let Some(dir) = exe_dir() {
                autoupdate::tidy_mcp_after_update(&dir);
            }
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
                        if notify(&app, kind, &detail) == Outcome::Later {
                            break; // one per gap; the rest comes on a later check
                        }
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
                // A release found earlier (also before a restart) is downloaded when installing
                // is on; a downloaded one installs when the PC is idle or locked.
                if now - last_install_check >= 30 {
                    last_install_check = now;
                    let up = update_status(&state);
                    if let (true, Some(v), true) = (up.available, up.latest_version.as_deref(), state.settings().auto_install_updates) {
                        start_download(&app, v);
                    }
                    maybe_install(&app, &state);
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
    /// Set when settings.json could not be read at startup: where the unreadable file was kept.
    damaged_settings_copy: Option<String>,
    /// The download and install of an update.
    install: autoupdate::InstallStatus,
}

#[tauri::command]
pub fn platform_info(state: S) -> PlatformInfo {
    let settings = state.settings();
    PlatformInfo {
        version: env!("CARGO_PKG_VERSION").into(),
        pause_options: pause::choices(&settings).into_iter().map(|c| PauseOption { id: c.id(), label: c.label() }).collect(),
        minute_options: pause::MINUTE_OPTIONS.to_vec(),
        update: update_status(&state),
        damaged_settings_copy: kv_get(&state, K_SETTINGS_DAMAGED),
        install: install_status(),
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

/// "Install and restart now" in Settings. On success Daytrace quits for the installer.
#[tauri::command]
pub async fn platform_install_update(app: AppHandle) -> CmdResult<()> {
    install_now(&app).await.map_err(err)
}

/// The notes to show once after an update, or None.
#[tauri::command]
pub async fn platform_whats_new(state: S<'_>) -> CmdResult<Option<whats_new::WhatsNew>> {
    let st = state.inner().clone();
    let current = env!("CARGO_PKG_VERSION");
    if !whats_new::due(kv_get(&st, K_WHATS_NEW).as_deref(), current, st.settings().onboarding_completed) {
        return Ok(None);
    }
    let status = update_status(&st);
    let fetched = if whats_new::stored_has_notes(&status, current) { None } else { Some(update::fetch_github_tag(current).await) };
    let found = whats_new::resolve(current, &status, fetched, whats_new::CHANGELOG);
    if found.is_none() {
        // No notes anywhere: nothing to show for this version.
        kv_set(&st, K_WHATS_NEW, current);
    }
    Ok(found)
}

/// Records that the notes of `version` were shown.
#[tauri::command]
pub fn platform_whats_new_seen(state: S, version: String) {
    kv_set(&state, K_WHATS_NEW, version.trim());
}

/// "Copy debug info": recent batches, model calls and the end of the log, with keys, the user
/// folder and titles taken out.
#[tauri::command]
pub async fn platform_debug_info(state: S<'_>) -> CmdResult<String> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let s = st.settings();
        let secrets: Vec<String> = [
            crate::config::stored_key(s.provider, &s.base_url),
            s.fallback_provider.and_then(|k| crate::config::stored_fallback_key(k, &s.fallback_base_url)),
        ]
        .into_iter()
        .flatten()
        .collect();
        let env = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
        let r = debug_info::Redactions { secrets, home: env("USERPROFILE").or_else(|| env("HOME")), user: env("USERNAME").or_else(|| env("USER")), titles: vec![] };
        let up = update_status(&st);
        let inst = install_status();
        let extra = vec![
            format!(
                "Update check: latest {} · available {} · last check {} · error {}",
                up.latest_version.as_deref().unwrap_or("unknown"),
                up.available,
                up.attempted_at.or(up.checked_at).map(|t| crate::timeutil::local(t).format("%Y-%m-%d %H:%M").to_string()).unwrap_or_else(|| "never".into()),
                up.error.as_deref().unwrap_or("none")
            ),
            format!("Update install: {:?} {} {}", inst.stage, inst.version.as_deref().unwrap_or(""), inst.error.as_deref().unwrap_or("")).trim_end().to_string(),
        ];
        debug_info::build(&st, &st.paths.root.join("daytrace.log"), now_ts(), r, &extra)
    })
    .await
    .map_err(err)?
    .map_err(err)
}

/// Opens a web link from the release notes in the browser.
#[tauri::command]
pub fn platform_open_link(app: AppHandle, url: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let parsed = reqwest::Url::parse(url.trim()).map_err(|_| "That link is not a valid web address.".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only web links can be opened.".into());
    }
    app.opener().open_url(parsed.as_str(), None::<&str>).map_err(err)
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
    use crate::config::{Paths, ProviderKind};

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let st = AppState::new(Paths::new(dir.path().to_path_buf()).unwrap()).unwrap();
        (dir, st)
    }

    #[test]
    fn detects_existing_installs() {
        assert_eq!(classify_install(None, false), Install::Fresh);
        assert_eq!(classify_install(None, true), Install::Existing, "never saved its settings but has recordings");
        assert_eq!(classify_install(Some(r#"{"recordingEnabled":true}"#), false), Install::Existing);
        assert_eq!(classify_install(Some(r#"{"recordingEnabled":true,"onboardingCompleted":false}"#), true), Install::Current);
        assert_eq!(classify_install(Some("{broken"), true), Install::Damaged);
        assert_eq!(classify_install(Some(r#"{"provider":"gemini_native"}"#), true), Install::Damaged, "a value this version can't read");
        assert_eq!(classify_install(Some(""), false), Install::Damaged);
    }

    #[test]
    fn damaged_settings_are_kept_and_recording_waits() {
        let (dir, st) = state();
        let broken = r#"{"provider":"anthropic","blockedApps":["keepass","bank"],"retentionDays":3,"#;
        std::fs::write(dir.path().join("settings.json"), broken).unwrap();
        let st = { drop(st); AppState::new(Paths::new(dir.path().to_path_buf()).unwrap()).unwrap() };
        init(&st);
        assert_eq!(std::fs::read_to_string(dir.path().join("settings.json.bad")).unwrap(), broken);
        assert_eq!(std::fs::read_to_string(dir.path().join("settings.json")).unwrap(), broken, "not overwritten at startup");
        assert!(!st.settings().onboarding_completed);
        assert!(!pause::recording_allowed(&st.settings()), "recording waits in the setup");
        assert!(kv_get(&st, K_SETTINGS_DAMAGED).unwrap().ends_with("settings.json.bad"));
        // Finishing the setup writes a good file; the copy stays.
        finish_onboarding(&st, true).unwrap();
        assert!(kv_get(&st, K_SETTINGS_DAMAGED).is_none());
        assert!(Settings::load(&st.paths.settings).onboarding_completed);
        assert!(dir.path().join("settings.json.bad").exists());
    }

    #[test]
    fn install_with_recordings_but_no_settings_keeps_recording() {
        let (dir, st) = state();
        st.db.insert_screenshot(1_800_000_000, "", 0, 0, Some("code"), None, None, None).unwrap();
        assert!(!dir.path().join("settings.json").exists());
        init(&st);
        assert!(st.settings().onboarding_completed);
        assert!(pause::recording_allowed(&st.settings()));
        assert!(Settings::load(&st.paths.settings).onboarding_completed);
    }

    #[test]
    fn concurrent_saves_leave_a_whole_file_that_matches_memory() {
        let (_d, st) = state();
        let st = std::sync::Arc::new(st);
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let st = st.clone();
                std::thread::spawn(move || {
                    for j in 0..25 {
                        update_settings(&st, |s| s.retention_days = 1 + ((i * 25 + j) % 300) as u32).unwrap();
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let raw = std::fs::read_to_string(&st.paths.settings).unwrap();
        let on_disk: Settings = serde_json::from_str(&raw).expect("a whole file");
        assert_eq!(on_disk.retention_days, st.settings().retention_days, "the last save in memory is the one on disk");
        let leftovers = std::fs::read_dir(st.paths.settings.parent().unwrap()).unwrap().flatten().filter(|e| e.file_name().to_string_lossy().ends_with(".tmp")).count();
        assert_eq!(leftovers, 0);
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

        // A newer release, until it was announced.
        let st_up = UpdateStatus { current_version: env!("CARGO_PKG_VERSION").into(), latest_version: Some("999.0.0".into()), available: true, ..Default::default() };
        kv_set(&st, K_UPDATE, &serde_json::to_string(&st_up).unwrap());
        assert!(detect(&st, &mut w, now).contains(&(Kind::Update, "999.0.0".to_string())));
        assert_eq!(admit(&st, Kind::Update, "999.0.0", now, false), Outcome::Shown);
        assert!(!detect(&st, &mut w, now).iter().any(|(k, _)| *k == Kind::Update));
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
        // Held back by the gap: offered again.
        settle(&st, Kind::RecapReady, &target, Outcome::Later);
        assert_eq!(detect(&st, &mut w, now + 30), vec![(Kind::RecapReady, target.clone())]);
        settle(&st, Kind::RecapReady, &target, Outcome::Shown);
        assert!(detect(&st, &mut w, now + 60).is_empty(), "announced once");
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
        settle(&st, Kind::FirstCards, "", Outcome::Skipped);
        assert!(detect(&st, &mut w, now).is_empty());
    }

    #[test]
    fn admits_within_limits() {
        let (_d, st) = state();
        // Local noon, so the hours added below stay inside one logical day (4 AM to 4 AM) in
        // every time zone.
        let now = {
            use chrono::TimeZone;
            chrono::Local.with_ymd_and_hms(2027, 1, 15, 12, 0, 0).earliest().unwrap().timestamp()
        };
        assert_eq!(admit(&st, Kind::MissingKey, "", now, false), Outcome::Shown);
        assert_eq!(admit(&st, Kind::MissingKey, "", now + 3600, false), Outcome::Skipped, "once a day");
        assert_eq!(admit(&st, Kind::RecordingProblem, "", now + 10, false), Outcome::Later, "minimum gap");
        assert_eq!(admit(&st, Kind::RecordingProblem, "", now + 3600, true), Outcome::Skipped, "window in front");
        assert_eq!(admit(&st, Kind::RecordingProblem, "", now + 3600, false), Outcome::Shown);
        update_settings(&st, |s| s.notifications.updates = false).unwrap();
        assert_eq!(admit(&st, Kind::Update, "0.3.0", now + 7200, false), Outcome::Skipped, "turned off");
        update_settings(&st, |s| s.notifications.updates = true).unwrap();
        assert_eq!(admit(&st, Kind::Update, "0.3.0", now + 7200, false), Outcome::Shown);
        assert_eq!(admit(&st, Kind::Update, "0.3.0", now + 3 * 86400, false), Outcome::Skipped, "once per version");
        assert_eq!(admit(&st, Kind::Test, "", now + 7201, true), Outcome::Shown, "test always shows");
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
