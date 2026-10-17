//! What the tray shows: the state of recording mapped to an icon, a tooltip and the status line at
//! the top of the menu (Dayflow's StatusBarController.menuIcon / menuNeedsUpdate). Pure, so the
//! mapping is tested without a tray.

use super::pause::{self, Mode};
use crate::capture::RecorderStatus;
use crate::config::Settings;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IconKind {
    /// Recording (also while locked or away: recording is on and resumes by itself).
    Normal,
    /// Paused by the user, or waiting for setup.
    Paused,
    /// The screen cannot be captured.
    Error,
}

/// Everything the tray displays; the tray is only updated when this changes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrayView {
    pub icon: IconKind,
    pub tooltip: String,
    /// First, disabled line of the menu.
    pub status: String,
    /// Whether the menu offers "Pause for…" (true) or "Resume recording" (false).
    pub can_pause: bool,
    /// Whether "Resume recording" is offered.
    pub can_resume: bool,
    /// Ids and labels of the pause choices.
    pub pause_choices: Vec<(String, String)>,
    /// "Version X available" item, when a newer release is known.
    pub update: Option<String>,
}

/// Windows truncates tray tooltips at 127 characters.
const TOOLTIP_MAX: usize = 127;

fn clip(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n - 1).collect();
        out.push('…');
        out
    }
}

/// The status line for the current state. `clock` formats a Unix time as "3:15 PM".
pub fn status_line(settings: &Settings, recorder: &RecorderStatus, clock: &dyn Fn(i64) -> String) -> (IconKind, String) {
    match pause::mode(settings) {
        Mode::Setup => (IconKind::Paused, "Finish setup to start recording".into()),
        Mode::PausedUntil(t) => (IconKind::Paused, format!("Paused until {}", clock(t))),
        Mode::PausedIndefinite => (IconKind::Paused, "Recording paused".into()),
        Mode::Active => match recorder {
            RecorderStatus::Error(_) => (IconKind::Error, "Can’t capture the screen".into()),
            RecorderStatus::PausedLocked => (IconKind::Normal, "Paused while locked".into()),
            RecorderStatus::PausedIdle => (IconKind::Normal, "Paused, you’re away".into()),
            RecorderStatus::Recording | RecorderStatus::Off => (IconKind::Normal, "Recording".into()),
        },
    }
}

pub fn view(settings: &Settings, recorder: &RecorderStatus, update: Option<&str>, clock: &dyn Fn(i64) -> String) -> TrayView {
    let (icon, status) = status_line(settings, recorder, clock);
    let mode = pause::mode(settings);
    let mut tooltip = format!("Daytrace: {}", status.to_lowercase_first());
    if let (Mode::Active, RecorderStatus::Error(detail)) = (mode, recorder) {
        tooltip = format!("{tooltip} ({detail})");
    }
    TrayView {
        icon,
        tooltip: clip(&tooltip, TOOLTIP_MAX),
        status,
        can_pause: mode == Mode::Active,
        can_resume: matches!(mode, Mode::PausedUntil(_) | Mode::PausedIndefinite),
        pause_choices: pause::choices(settings).into_iter().map(|c| (c.id(), c.label())).collect(),
        update: update.map(|v| format!("Version {v} available")),
    }
}

trait LowerFirst {
    fn to_lowercase_first(&self) -> String;
}

impl LowerFirst for String {
    /// "Paused until 3:15 PM" → "paused until 3:15 PM" (only the first letter changes).
    fn to_lowercase_first(&self) -> String {
        let mut c = self.chars();
        match c.next() {
            Some(f) => f.to_lowercase().collect::<String>() + c.as_str(),
            None => String::new(),
        }
    }
}

/// Makes the icon variant from the app icon's RGBA pixels: Paused is grey and faded, Error gets
/// a red dot in the bottom-right corner.
pub fn tint(rgba: &[u8], width: u32, height: u32, kind: IconKind) -> Vec<u8> {
    let mut out = rgba.to_vec();
    match kind {
        IconKind::Normal => {}
        IconKind::Paused => {
            for px in out.chunks_exact_mut(4) {
                let l = (0.299 * px[0] as f32 + 0.587 * px[1] as f32 + 0.114 * px[2] as f32).round() as u8;
                px[0] = l;
                px[1] = l;
                px[2] = l;
                px[3] = (px[3] as f32 * 0.55).round() as u8;
            }
        }
        IconKind::Error => {
            let size = width.min(height) as f32;
            let r = size * 0.22;
            let ring = (size * 0.06).max(1.0);
            let (cx, cy) = (width as f32 - r - ring, height as f32 - r - ring);
            for y in 0..height {
                for x in 0..width {
                    let d = ((x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2)).sqrt();
                    let i = ((y * width + x) * 4) as usize;
                    if d <= r {
                        out[i..i + 4].copy_from_slice(&[214, 48, 49, 255]);
                    } else if d <= r + ring {
                        out[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
                    }
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clock(t: i64) -> String {
        if t == 1000 { "3:15 PM".into() } else { format!("t{t}") }
    }

    fn base() -> Settings {
        Settings { onboarding_completed: true, ..Settings::default() }
    }

    #[test]
    fn maps_states() {
        let s = base();
        let v = view(&s, &RecorderStatus::Recording, None, &clock);
        assert_eq!((v.icon, v.status.as_str(), v.tooltip.as_str()), (IconKind::Normal, "Recording", "Daytrace: recording"));
        assert!(v.can_pause && !v.can_resume);
        assert_eq!(v.pause_choices.len(), 5);
        assert_eq!(v.pause_choices[0], ("15".to_string(), "15 minutes".to_string()));
        assert_eq!(v.pause_choices[3].1, "Until tomorrow (4 AM)");

        let v = view(&s, &RecorderStatus::PausedIdle, None, &clock);
        assert_eq!((v.icon, v.status.as_str()), (IconKind::Normal, "Paused, you’re away"));
        let v = view(&s, &RecorderStatus::PausedLocked, None, &clock);
        assert_eq!(v.status, "Paused while locked");

        let v = view(&s, &RecorderStatus::Error("no monitors found".into()), None, &clock);
        assert_eq!(v.icon, IconKind::Error);
        assert_eq!(v.tooltip, "Daytrace: can’t capture the screen (no monitors found)");
        assert!(v.can_pause);
    }

    #[test]
    fn maps_pauses() {
        let mut s = base();
        s.recording_enabled = false;
        s.pause_until = Some(1000);
        // The recorder may still report its last state for a moment; the pause wins.
        let v = view(&s, &RecorderStatus::Recording, None, &clock);
        assert_eq!((v.icon, v.status.as_str()), (IconKind::Paused, "Paused until 3:15 PM"));
        assert_eq!(v.tooltip, "Daytrace: paused until 3:15 PM");
        assert!(!v.can_pause && v.can_resume);

        s.pause_until = None;
        let v = view(&s, &RecorderStatus::Off, None, &clock);
        assert_eq!((v.icon, v.status.as_str()), (IconKind::Paused, "Recording paused"));
        assert!(v.can_resume);

        let s = Settings::default();
        let v = view(&s, &RecorderStatus::Off, None, &clock);
        assert_eq!(v.status, "Finish setup to start recording");
        assert!(!v.can_pause && !v.can_resume);
    }

    #[test]
    fn shows_update_and_clips_tooltip() {
        let s = base();
        let v = view(&s, &RecorderStatus::Recording, Some("0.3.0"), &clock);
        assert_eq!(v.update.as_deref(), Some("Version 0.3.0 available"));
        let v = view(&s, &RecorderStatus::Error("x".repeat(300)), None, &clock);
        assert_eq!(v.tooltip.chars().count(), TOOLTIP_MAX);
        assert!(v.tooltip.ends_with('…'));
    }

    #[test]
    fn tints_icons() {
        let px = [200u8, 100, 50, 255].repeat(32 * 32);
        assert_eq!(tint(&px, 32, 32, IconKind::Normal), px);
        let grey = tint(&px, 32, 32, IconKind::Paused);
        assert_eq!(&grey[..4], &[124, 124, 124, 140]);
        let err = tint(&px, 32, 32, IconKind::Error);
        assert_eq!(&err[..4], &px[..4], "top-left unchanged");
        let i = ((26 * 32 + 26) * 4) as usize;
        assert_eq!(&err[i..i + 4], &[214, 48, 49, 255], "red dot bottom-right");
    }
}
