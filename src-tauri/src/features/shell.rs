//! Small commands for the window shell (the rail and the taskbar button), owned by the shell UI.
//!
//! - The "new recap" badge on Daily (Dayflow NotificationBadgeManager: a dot on the Daily sidebar
//!   item while a recap is ready and Daily has not been opened since; cleared when Daily opens).
//!   A recap counts as new while today's recap was created after Daily was last opened.
//! - The taskbar overlay icon that mirrors that dot (Dayflow's Dock badge; Windows only).
//! - Opening a few fixed web pages a fix button points to (the Claude Console billing and API key
//!   pages). Only these addresses can be opened; nothing comes from the UI but a name.

use crate::features::platform_extras::{kv_get, kv_set};
use crate::state::AppState;
use crate::timeutil::{logical_day, now_ts};
use rusqlite::OptionalExtension;
use serde::Serialize;
use std::sync::Arc;

type S<'a> = tauri::State<'a, Arc<AppState>>;

/// When Daily was last opened (Unix seconds), in the platform_state table.
const K_RECAP_OPENED: &str = "shell_recap_opened_at";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShellBadges {
    /// The day of a recap that is new (created after Daily was last opened), else None.
    pub recap_day: Option<String>,
}

fn recap_created_at(state: &AppState, target: &str) -> Option<i64> {
    let c = state.db.conn();
    c.query_row("SELECT created_at FROM daily_recaps WHERE target_day = ?", [target], |r| r.get::<_, i64>(0)).optional().ok().flatten()
}

/// Today's recap when it was created after Daily was last opened.
pub fn badges(state: &AppState, now: i64) -> ShellBadges {
    let target = logical_day(now);
    let opened: i64 = kv_get(state, K_RECAP_OPENED).and_then(|v| v.parse().ok()).unwrap_or(0);
    let recap_day = recap_created_at(state, &target).filter(|created| *created > opened).map(|_| target);
    ShellBadges { recap_day }
}

pub fn recap_opened(state: &AppState, now: i64) {
    kv_set(state, K_RECAP_OPENED, &now.to_string());
}

/// The fixed pages a fix button may open.
fn link_for(target: &str) -> Option<&'static str> {
    match target {
        "console_billing" => Some("https://console.anthropic.com/settings/billing"),
        "console_keys" => Some("https://console.anthropic.com/settings/keys"),
        _ => None,
    }
}

/// A 32×32 RGBA dot for the taskbar overlay: an ink disc with a white ring, readable on light
/// and dark taskbars.
pub fn overlay_dot() -> Vec<u8> {
    const N: usize = 32;
    let mut px = vec![0u8; N * N * 4];
    let c = (N as f32 - 1.0) / 2.0;
    for y in 0..N {
        for x in 0..N {
            let d = ((x as f32 - c).powi(2) + (y as f32 - c).powi(2)).sqrt();
            let (rgb, a): ([u8; 3], f32) = if d <= 10.5 {
                ([0x1F, 0x23, 0x2B], 1.0)
            } else if d <= 15.0 {
                ([0xFF, 0xFF, 0xFF], 1.0)
            } else if d <= 16.0 {
                ([0xFF, 0xFF, 0xFF], 16.0 - d)
            } else {
                ([0, 0, 0], 0.0)
            };
            let i = (y * N + x) * 4;
            px[i..i + 3].copy_from_slice(&rgb);
            px[i + 3] = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    px
}

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

// ---------------------------------------------------------------- commands (// shell commands)

#[tauri::command]
pub fn shell_badges(state: S) -> ShellBadges {
    badges(&state, now_ts())
}

/// Daily was opened: the recap badge clears until the next recap.
#[tauri::command]
pub fn shell_recap_opened(state: S) {
    recap_opened(&state, now_ts());
}

/// Shows or removes the dot on the taskbar button (Windows only; elsewhere nothing happens).
#[tauri::command]
pub fn shell_set_overlay(app: tauri::AppHandle, on: bool) -> Result<(), String> {
    #[cfg(windows)]
    {
        use tauri::Manager;
        if let Some(w) = app.get_webview_window("main") {
            let icon = if on { Some(tauri::image::Image::new_owned(overlay_dot(), 32, 32)) } else { None };
            w.set_overlay_icon(icon).map_err(err)?;
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (app, on);
    }
    Ok(())
}

/// Opens one of the fixed help pages (see `link_for`) in the default browser.
#[tauri::command]
pub fn shell_open_link(app: tauri::AppHandle, target: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let url = link_for(&target).ok_or_else(|| format!("Unknown page: {target}"))?;
    app.opener().open_url(url, None::<&str>).map_err(err)
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

    fn add_recap(st: &AppState, day: &str, created: i64) {
        st.db
            .conn()
            .execute("INSERT INTO daily_recaps (target_day, source_day, created_at, updated_at) VALUES (?1, ?1, ?2, ?2)", rusqlite::params![day, created])
            .unwrap();
    }

    #[test]
    fn recap_badge_until_daily_is_opened() {
        let (_d, st) = state();
        let now = 1_792_000_000; // a fixed time
        let today = logical_day(now);
        assert_eq!(badges(&st, now).recap_day, None, "no recap yet");

        add_recap(&st, &today, now - 600);
        assert_eq!(badges(&st, now).recap_day.as_deref(), Some(today.as_str()));

        recap_opened(&st, now);
        assert_eq!(badges(&st, now).recap_day, None, "opened after the recap was written");
    }

    #[test]
    fn daily_opened_before_the_recap_still_shows_it() {
        let (_d, st) = state();
        let now = 1_792_000_000;
        let today = logical_day(now);
        recap_opened(&st, now - 3600);
        add_recap(&st, &today, now - 60);
        assert_eq!(badges(&st, now).recap_day.as_deref(), Some(today.as_str()));
    }

    #[test]
    fn yesterdays_recap_is_not_new_today() {
        let (_d, st) = state();
        let now = 1_792_000_000;
        add_recap(&st, &crate::timeutil::logical_day(now - 86_400), now - 86_000);
        assert_eq!(badges(&st, now).recap_day, None);
    }

    #[test]
    fn only_fixed_links_open() {
        assert!(link_for("console_billing").unwrap().starts_with("https://console.anthropic.com/"));
        assert!(link_for("https://example.com").is_none());
    }

    #[test]
    fn overlay_dot_is_a_ringed_disc() {
        let px = overlay_dot();
        assert_eq!(px.len(), 32 * 32 * 4);
        let at = |x: usize, y: usize| &px[(y * 32 + x) * 4..(y * 32 + x) * 4 + 4];
        assert_eq!(at(16, 16)[3], 255, "center is opaque");
        assert_eq!(&at(16, 16)[..3], &[0x1F, 0x23, 0x2B]);
        assert_eq!(&at(16, 3)[..3], &[0xFF, 0xFF, 0xFF], "white ring");
        assert_eq!(at(0, 0)[3], 0, "corners are transparent");
    }
}
