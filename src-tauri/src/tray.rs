use crate::state::AppState;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

const TRAY_ID: &str = "main";

/// Show the main window, creating it if needed. The window (and its WebView2 processes) is
/// destroyed when closed, so nothing of the UI runs while Daytrace sits in the tray.
pub fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let built = tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::App("index.html".into()))
        .title("Daytrace")
        .inner_size(1180.0, 780.0)
        .min_inner_size(760.0, 520.0)
        .build();
    match built {
        Ok(w) => {
            let _ = w.set_focus();
        }
        Err(e) => log::error!("could not open the window: {e}"),
    }
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let state = app.state::<Arc<AppState>>();
    let recording = state.settings.read().unwrap().recording_enabled;
    let open = MenuItem::with_id(app, "open", "Open Daytrace", true, None::<&str>)?;
    let toggle = MenuItem::with_id(
        app,
        "toggle",
        if recording { "Pause recording" } else { "Resume recording" },
        true,
        None::<&str>,
    )?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Daytrace", true, None::<&str>)?;
    Menu::with_items(app, &[&open, &toggle, &sep, &quit])
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(app.default_window_icon().cloned().expect("app icon"))
        .tooltip("Daytrace")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => show_main(app),
            "toggle" => {
                let state = app.state::<Arc<AppState>>();
                let s = {
                    let mut g = state.settings.write().unwrap();
                    g.recording_enabled = !g.recording_enabled;
                    g.clone()
                };
                let _ = s.save(&state.paths.settings);
                let _ = tauri::Emitter::emit(app, "settings-changed", ());
                refresh(app);
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show_main(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// Rebuild the menu so the pause/resume label matches the setting.
pub fn refresh(app: &AppHandle) {
    if let (Some(tray), Ok(menu)) = (app.tray_by_id(TRAY_ID), build_menu(app)) {
        let _ = tray.set_menu(Some(menu));
    }
}
