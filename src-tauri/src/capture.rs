//! Screenshot recorder: one JPEG of the monitor under the cursor every N seconds. Pauses while the
//! workstation is locked (resumes 5 s after unlock). Frames where a blocked app is in the
//! foreground are not captured at all; a row records that the app was hidden.

use crate::config::Settings;
use crate::platform;
use crate::state::AppState;
use anyhow::{anyhow, Result};
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "state", content = "detail")]
pub enum RecorderStatus {
    Off,
    Recording,
    PausedLocked,
    /// No input for longer than the idle guard allows; rows are kept, images are not.
    PausedIdle,
    Error(String),
}

/// What one tick stored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Outcome {
    Captured,
    PrivateApp,
    Idle,
}

const BACKOFF: [u64; 4] = [60, 120, 300, 600];

pub fn spawn(state: Arc<AppState>) {
    std::thread::Builder::new()
        .name("recorder".into())
        .spawn(move || run(state))
        .expect("spawn recorder");
}

fn set_status(state: &AppState, s: RecorderStatus) {
    let mut g = state.recorder.lock().unwrap();
    if *g != s {
        log::info!("recorder: {s:?}");
        *g = s;
    }
}

fn run(state: Arc<AppState>) {
    let mut failures: usize = 0;
    let mut locked_since: Option<Instant> = None;
    let mut unlocked_at: Option<Instant> = None;
    loop {
        let settings = state.settings.read().unwrap().clone();
        // Off when paused (also during a timed pause) or before first-run setup is finished.
        if !crate::features::platform_extras::pause::recording_allowed(&settings) {
            set_status(&state, RecorderStatus::Off);
            std::thread::sleep(Duration::from_millis(500));
            continue;
        }
        if platform::is_locked() {
            locked_since.get_or_insert_with(Instant::now);
            unlocked_at = None;
            set_status(&state, RecorderStatus::PausedLocked);
            std::thread::sleep(Duration::from_secs(1));
            continue;
        }
        if locked_since.take().is_some() {
            unlocked_at = Some(Instant::now());
        }
        if let Some(t) = unlocked_at {
            if t.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(250));
                continue;
            }
            unlocked_at = None;
        }

        let started = Instant::now();
        let mut was_idle = false;
        let wait = match capture_once(&state, &settings) {
            Ok(outcome) => {
                failures = 0;
                was_idle = outcome == Outcome::Idle;
                set_status(&state, if was_idle { RecorderStatus::PausedIdle } else { RecorderStatus::Recording });
                Duration::from_secs(settings.capture_interval_secs)
            }
            Err(e) => {
                log::error!("capture failed: {e:#}");
                let backoff = BACKOFF[failures.min(BACKOFF.len() - 1)];
                failures += 1;
                set_status(&state, RecorderStatus::Error(format!("{e}")));
                Duration::from_secs(backoff)
            }
        };
        // Sleep in small steps so toggling recording, locking or coming back to the PC takes
        // effect quickly.
        let deadline = started + wait;
        let enabled_now = || crate::features::platform_extras::pause::recording_allowed(&state.settings.read().unwrap());
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(250));
            if !enabled_now() || state.capture_now.swap(false, std::sync::atomic::Ordering::SeqCst) {
                break;
            }
            if was_idle && platform::idle_seconds() < 2 {
                break; // the user is back: capture right away
            }
        }
    }
}

/// The monitor with the active window (where the user works); without one, the monitor under
/// the mouse; otherwise the primary monitor.
fn pick_monitor(fg: &platform::ForegroundApp) -> Result<xcap::Monitor> {
    if let Some((x, y)) = fg.monitor_center {
        if let Ok(m) = xcap::Monitor::from_point(x, y) {
            return Ok(m);
        }
    }
    if let Some((x, y)) = platform::cursor_position() {
        if let Ok(m) = xcap::Monitor::from_point(x, y) {
            return Ok(m);
        }
    }
    let monitors = xcap::Monitor::all().map_err(|e| anyhow!("listing monitors: {e}"))?;
    let primary = monitors.iter().position(|m| m.is_primary().unwrap_or(false)).unwrap_or(0);
    monitors.into_iter().nth(primary).ok_or_else(|| anyhow!("no monitors found"))
}

/// Paint over private apps' windows that are visible on the captured monitor but not in front.
fn mask_blocked_windows(img: &mut image::RgbaImage, monitor: &xcap::Monitor, blocked: &[String]) {
    let rects = platform::blocked_window_rects(blocked);
    if rects.is_empty() {
        return;
    }
    let (Ok(mx), Ok(my), Ok(mw), Ok(mh)) = (monitor.x(), monitor.y(), monitor.width(), monitor.height()) else { return };
    // Monitor geometry may be logical while the image is physical; scale between them.
    let sx = img.width() as f64 / mw.max(1) as f64;
    let sy = img.height() as f64 / mh.max(1) as f64;
    for (l, t, r, b) in rects {
        let x0 = (((l - mx) as f64) * sx).floor().max(0.0) as u32;
        let y0 = (((t - my) as f64) * sy).floor().max(0.0) as u32;
        let x1 = ((((r - mx) as f64) * sx).ceil().max(0.0) as u32).min(img.width());
        let y1 = ((((b - my) as f64) * sy).ceil().max(0.0) as u32).min(img.height());
        if x1 <= x0 {
            continue;
        }
        let stride = img.width() as usize * 4;
        let buf: &mut [u8] = &mut *img;
        for y in y0..y1 {
            let row = &mut buf[y as usize * stride + x0 as usize * 4..y as usize * stride + x1 as usize * 4];
            for px in row.chunks_exact_mut(4) {
                px.copy_from_slice(&[40, 44, 43, 255]);
            }
        }
    }
}

pub fn capture_once(state: &AppState, settings: &Settings) -> Result<Outcome> {
    let now = chrono::Local::now();
    let ts = now.timestamp();
    let idle = platform::idle_seconds() as i64;

    // Idle guard: nobody is using the PC and no app keeps the display on (video), so store a
    // row without an image. Rows keep batching and Idle cards working; no image means nothing
    // to encode, store or send to a model.
    if settings.idle_pause_secs > 0 && idle as u64 >= settings.idle_pause_secs && !platform::display_required() {
        state.db.insert_screenshot(ts, "", 0, idle, None, None, None, Some(crate::db::SKIP_IDLE))?;
        return Ok(Outcome::Idle);
    }

    let fg = platform::foreground_app().unwrap_or_default();
    let app = (!fg.app.is_empty()).then_some(fg.app.as_str());
    let title = (!fg.title.is_empty()).then_some(fg.title.as_str());

    if let Some(blocked) = platform::blocked_match(&fg.app, &settings.blocked_apps) {
        log::debug!("foreground app {} is blocked ({blocked}); not capturing", fg.app);
        state.db.insert_screenshot(ts, "", 0, idle, app, None, Some(&fg.app), None)?;
        return Ok(Outcome::PrivateApp);
    }

    let monitor = pick_monitor(&fg)?;
    let mut img = monitor.capture_image().map_err(|e| anyhow!("capture: {e}"))?;
    mask_blocked_windows(&mut img, &monitor, &settings.blocked_apps);
    let rgb = crate::imaging::downscale_rgba(img, settings.capture_height);

    let dir = state.paths.screenshots.join(now.format("%Y-%m-%d").to_string());
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.jpg", now.format("%H%M%S_%3f")));
    let buf = crate::imaging::encode_jpeg(&rgb, settings.jpeg_quality)?;
    std::fs::write(&path, &buf)?;
    let exe = (!fg.path.is_empty()).then_some(fg.path.as_str());
    state
        .db
        .insert_screenshot_with_path(ts, &path.to_string_lossy(), buf.len() as i64, idle, app, title, None, None, exe)?;
    crate::features::icons::note_foreground(state, settings, &fg.app, &fg.path);
    Ok(Outcome::Captured)
}
