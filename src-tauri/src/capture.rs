//! Screenshot recorder: one JPEG of the monitor under the cursor every N seconds. Pauses while the
//! workstation is locked (resumes 5 s after unlock). Frames where a blocked app is in the
//! foreground are not captured at all; a row records that the app was hidden.

use crate::config::Settings;
use crate::platform;
use crate::state::AppState;
use anyhow::{anyhow, Result};
use image::imageops::FilterType;
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", tag = "state", content = "detail")]
pub enum RecorderStatus {
    Off,
    Recording,
    PausedLocked,
    Error(String),
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
        if !settings.recording_enabled {
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
        let wait = match capture_once(&state, &settings) {
            Ok(()) => {
                failures = 0;
                set_status(&state, RecorderStatus::Recording);
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
        // Sleep in small steps so toggling recording or locking takes effect quickly.
        let deadline = started + wait;
        let enabled_now = || state.settings.read().unwrap().recording_enabled;
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(250));
            if !enabled_now() || state.capture_now.swap(false, std::sync::atomic::Ordering::SeqCst) {
                break;
            }
        }
    }
}

fn pick_monitor() -> Result<xcap::Monitor> {
    if let Some((x, y)) = platform::cursor_position() {
        if let Ok(m) = xcap::Monitor::from_point(x, y) {
            return Ok(m);
        }
    }
    let monitors = xcap::Monitor::all().map_err(|e| anyhow!("listing monitors: {e}"))?;
    let primary = monitors.iter().position(|m| m.is_primary().unwrap_or(false)).unwrap_or(0);
    monitors.into_iter().nth(primary).ok_or_else(|| anyhow!("no monitors found"))
}

pub fn capture_once(state: &AppState, settings: &Settings) -> Result<()> {
    let now = chrono::Local::now();
    let ts = now.timestamp();
    let idle = platform::idle_seconds() as i64;
    let fg = platform::foreground_app().unwrap_or_default();
    let app = (!fg.app.is_empty()).then_some(fg.app.as_str());
    let title = (!fg.title.is_empty()).then_some(fg.title.as_str());

    if let Some(blocked) = platform::blocked_match(&fg.app, &settings.blocked_apps) {
        log::debug!("foreground app {} is blocked ({blocked}); not capturing", fg.app);
        state.db.insert_screenshot(ts, "", 0, idle, app, None, Some(&fg.app))?;
        return Ok(());
    }

    let monitor = pick_monitor()?;
    let img = monitor.capture_image().map_err(|e| anyhow!("capture: {e}"))?;
    let target_h = settings.capture_height;
    let img = image::DynamicImage::ImageRgba8(img);
    let img = if img.height() > target_h {
        let w = ((img.width() as f64 * target_h as f64 / img.height() as f64).round() as u32).max(1);
        img.resize_exact(w + (w % 2), target_h, FilterType::Triangle)
    } else {
        img
    };
    let rgb = img.to_rgb8();

    let dir = state.paths.screenshots.join(now.format("%Y-%m-%d").to_string());
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.jpg", now.format("%H%M%S_%3f")));
    let mut buf = Vec::with_capacity(300_000);
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, settings.jpeg_quality).encode_image(&rgb)?;
    std::fs::write(&path, &buf)?;
    state
        .db
        .insert_screenshot(ts, &path.to_string_lossy(), buf.len() as i64, idle, app, title, None)?;
    Ok(())
}
