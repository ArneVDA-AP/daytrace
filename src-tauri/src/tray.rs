//! The tray icon (Dayflow's StatusBarController): the icon and tooltip show whether Daytrace is
//! recording, paused or failing; the menu offers Open, Pause for… or Resume, the screenshots
//! folder, the update check and Quit. `sync` is called once a second by the platform loop and
//! only touches the tray when what it shows has changed.

use crate::features::platform_extras::{self, pause::PauseChoice, tray_state};
use crate::state::AppState;
use std::sync::{Arc, Mutex};
use tauri::image::Image;
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, Wry};

const TRAY_ID: &str = "main";

/// What the tray shows right now.
static SHOWN: Mutex<Option<tray_state::TrayView>> = Mutex::new(None);
/// Newer release to offer in the menu. Kept here so the once-a-second check on the main thread
/// never waits for the database.
static LATEST: Mutex<Option<String>> = Mutex::new(None);

/// Sets the newer version the menu offers (None: no update known).
pub fn set_update(latest: Option<String>) {
    *LATEST.lock().unwrap() = latest;
}

/// Show the main window, creating it if needed. The window (and its WebView2 processes) is
/// destroyed when closed, so nothing of the UI runs while Daytrace sits in the tray.
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let appearance = app.state::<Arc<AppState>>().settings().appearance;
    // Built hidden so the window and webview get the theme's background colour before the first
    // frame: no white flash in dark mode. The resolved theme (the Windows setting when following
    // it) is only known once the window exists.
    let links = app.clone();
    let built = tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
        .title("Daytrace")
        .inner_size(1180.0, 780.0)
        .min_inner_size(760.0, 520.0)
        .theme(platform_extras::window_theme(appearance))
        .visible(false)
        // Links that ask for a new window can only come from the Agents dashboard frame: its web
        // links open in the default browser; no window ever opens (features/agents.rs).
        .on_new_window(move |url, _| {
            crate::features::agents::on_new_window(&links, url.as_str());
            tauri::webview::NewWindowResponse::Deny
        })
        .build();
    match built {
        Ok(w) => {
            crate::features::agents::watch_frame_keys(&w);
            let dark = w.theme().map(|t| t == tauri::Theme::Dark).unwrap_or(false);
            let _ = w.set_background_color(Some(window_background(dark)));
            let _ = w.show();
            let _ = w.set_focus();
        }
        Err(e) => log::error!("could not open the window: {e}"),
    }
}

/// The app canvas colour (--background in src/design/tokens.css): #f4f6f8 light, #0f1113 dark.
fn window_background(dark: bool) -> tauri::window::Color {
    if dark {
        tauri::window::Color(0x0f, 0x11, 0x13, 0xff)
    } else {
        tauri::window::Color(0xf4, 0xf6, 0xf8, 0xff)
    }
}

fn current_view(app: &AppHandle) -> tray_state::TrayView {
    let state = app.state::<Arc<AppState>>();
    let settings = state.settings();
    let recorder = state.recorder.lock().unwrap().clone();
    let latest = LATEST.lock().unwrap().clone();
    tray_state::view(&settings, &recorder, latest.as_deref(), &crate::timeutil::fmt_clock)
}

fn icon(app: &AppHandle, kind: tray_state::IconKind) -> Option<Image<'static>> {
    let base = app.default_window_icon()?;
    let (w, h) = (base.width(), base.height());
    Some(Image::new_owned(tray_state::tint(base.rgba(), w, h, kind), w, h))
}

fn build_menu(app: &AppHandle, v: &tray_state::TrayView) -> tauri::Result<Menu<Wry>> {
    let status = MenuItem::with_id(app, "status", &v.status, false, None::<&str>)?;
    let open = MenuItem::with_id(app, "open", "Open Daytrace", true, None::<&str>)?;
    let choices = v
        .pause_choices
        .iter()
        .map(|(id, label)| MenuItem::with_id(app, format!("pause:{id}"), label, true, None::<&str>))
        .collect::<tauri::Result<Vec<_>>>()?;
    let choice_refs: Vec<&dyn IsMenuItem<Wry>> = choices.iter().map(|i| i as &dyn IsMenuItem<Wry>).collect();
    let pause = Submenu::with_id_and_items(app, "pause", "Pause for", true, &choice_refs)?;
    let resume = MenuItem::with_id(app, "resume", "Resume recording", true, None::<&str>)?;
    let shots = MenuItem::with_id(app, "open_shots", "Open screenshots folder", true, None::<&str>)?;
    let update = match &v.update {
        Some(label) => MenuItem::with_id(app, "open_release", label, true, None::<&str>)?,
        None => MenuItem::with_id(app, "check_updates", "Check for updates", true, None::<&str>)?,
    };
    let quit = MenuItem::with_id(app, "quit", "Quit Daytrace", true, None::<&str>)?;
    let (sep1, sep2, sep3) = (PredefinedMenuItem::separator(app)?, PredefinedMenuItem::separator(app)?, PredefinedMenuItem::separator(app)?);

    let mut items: Vec<&dyn IsMenuItem<Wry>> = vec![&status, &sep1, &open];
    if v.can_pause {
        items.push(&pause);
    }
    if v.can_resume {
        items.push(&resume);
    }
    items.extend([&sep2 as &dyn IsMenuItem<Wry>, &shots, &update, &sep3, &quit]);
    Menu::with_items(app, &items)
}

fn on_menu(app: &AppHandle, id: &str) {
    let result = match id {
        "open" => {
            show_main(app);
            Ok(())
        }
        "resume" => platform_extras::resume_recording(app),
        "open_shots" => {
            platform_extras::open_screenshots(app);
            Ok(())
        }
        "open_release" => {
            platform_extras::open_release_page(app);
            Ok(())
        }
        "check_updates" => {
            let a = app.clone();
            tauri::async_runtime::spawn(async move {
                platform_extras::check_updates(&a, true).await;
            });
            Ok(())
        }
        "quit" => {
            app.exit(0);
            Ok(())
        }
        other => match other.strip_prefix("pause:").and_then(PauseChoice::parse) {
            Some(choice) => platform_extras::pause_recording(app, choice),
            None => Ok(()),
        },
    };
    if let Err(e) = result {
        log::error!("tray action {id} failed: {e:#}");
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    {
        let update = platform_extras::update_status(&app.state::<Arc<AppState>>());
        set_update(update.latest_version.filter(|_| update.available));
    }
    let v = current_view(app);
    let mut b = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(&v.tooltip)
        .menu(&build_menu(app, &v)?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        });
    if let Some(i) = icon(app, v.icon) {
        b = b.icon(i);
    }
    b.build(app)?;
    *SHOWN.lock().unwrap() = Some(v);
    Ok(())
}

/// Brings the tray up to date. Runs on the main thread: tray calls made elsewhere wait for the
/// main thread, so doing the work there keeps the comparison and the update in one place and
/// never blocks a caller.
fn apply(app: &AppHandle, force: bool) {
    let a = app.clone();
    let _ = app.run_on_main_thread(move || apply_now(&a, force));
}

fn apply_now(app: &AppHandle, force: bool) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else { return };
    let v = current_view(app);
    let mut shown = SHOWN.lock().unwrap();
    if !force && shown.as_ref() == Some(&v) {
        return;
    }
    let icon_changed = shown.as_ref().map(|s| s.icon) != Some(v.icon);
    match build_menu(app, &v) {
        Ok(menu) => {
            let _ = tray.set_menu(Some(menu));
        }
        Err(e) => log::warn!("tray menu: {e}"),
    }
    let _ = tray.set_tooltip(Some(&v.tooltip));
    if (icon_changed || force)
        && let Some(i) = icon(app, v.icon)
    {
        let _ = tray.set_icon(Some(i));
    }
    *shown = Some(v);
}

/// Rebuild the tray now (after a setting changed).
pub fn refresh(app: &AppHandle) {
    apply(app, true);
}

/// Update the tray if what it should show has changed.
pub fn sync(app: &AppHandle) {
    apply(app, false);
}
