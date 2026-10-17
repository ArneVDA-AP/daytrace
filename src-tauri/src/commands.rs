use crate::config::{default_base_url, default_model, ProviderKind, Settings};
use crate::db::StoredCard;
use crate::llm::client::{CallOpts, LlmClient, Part};
use crate::state::{AnalysisStatus, AppState};
use crate::timeutil::{logical_day, logical_day_bounds, now_ts};
use base64::Engine;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

type S<'a> = State<'a, Arc<AppState>>;
type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsDto {
    settings: Settings,
    has_api_key: bool,
    data_dir: String,
    /// The revision of `settings` (AppState::settings_revision); a save based on them passes it
    /// back as `baseRevision`.
    revision: u64,
}

#[tauri::command]
pub fn get_settings(state: S) -> SettingsDto {
    let (settings, revision) = state.settings_with_revision();
    SettingsDto {
        has_api_key: crate::config::stored_key(settings.provider, &settings.base_url).is_some(),
        settings,
        data_dir: state.paths.root.to_string_lossy().to_string(),
        revision,
    }
}

/// The code of the error `save_settings` gives for a save based on settings that changed since.
pub const SETTINGS_CHANGED: &str = "settings_changed";

/// Why `save_settings` refused. A stale save is an object, `{code: "settings_changed", message,
/// revision}`, so the window can re-read and apply its change again; anything else stays the
/// plain message every command gives.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum SaveError {
    Stale { code: &'static str, message: String, revision: u64 },
    Failed(String),
}

impl From<String> for SaveError {
    fn from(m: String) -> SaveError {
        SaveError::Failed(m)
    }
}

impl From<&str> for SaveError {
    fn from(m: &str) -> SaveError {
        SaveError::Failed(m.to_string())
    }
}

/// Writes settings from the window: refused when `base_revision` is given and the settings
/// changed after it (by the tray, an AI tool, the platform loop or another save); otherwise the
/// file, then memory, under the save lock like every other settings change.
pub fn store_settings(state: &AppState, mut settings: Settings, base_revision: Option<u64>) -> Result<u64, SaveError> {
    let _saving = crate::config::save_lock();
    let current = state.settings_revision();
    if base_revision.is_some_and(|base| base != current) {
        return Err(SaveError::Stale {
            code: SETTINGS_CHANGED,
            message: "The settings changed while this change was being saved. Try again.".into(),
            revision: current,
        });
    }
    crate::features::chat::clear_model_on_provider_change(&state.settings(), &mut settings);
    settings.save(&state.paths.settings).map_err(err)?;
    Ok(state.replace_settings(settings))
}

/// `base_revision`: the revision of the settings the change was made on (from `get_settings`).
/// Callers that leave it out are not checked, as before revisions existed.
#[tauri::command]
pub fn save_settings(app: tauri::AppHandle, state: S, settings: Settings, api_key: Option<String>, base_revision: Option<u64>) -> Result<SettingsDto, SaveError> {
    crate::features::insights::check_settings(&settings)?;
    let settings = settings.sanitized();
    // Everything that can refuse the request is checked before anything is written.
    let key = api_key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
    let slot = match &key {
        Some(_) => Some(crate::config::key_slot(settings.provider, &settings.base_url).ok_or("This provider does not use an API key")?),
        None => None,
    };
    let launch_at_login = settings.launch_at_login;
    store_settings(&state, settings, base_revision)?;
    if let (Some(slot), Some(k)) = (slot, key) {
        crate::platform::secret_set(&slot, &k).map_err(err)?;
    }
    {
        use tauri_plugin_autostart::ManagerExt;
        let al = app.autolaunch();
        let _ = if launch_at_login { al.enable() } else { al.disable() };
    }
    crate::tray::refresh(&app);
    Ok(get_settings(state))
}

#[tauri::command]
pub fn provider_defaults(provider: ProviderKind) -> (String, String) {
    (default_base_url(provider).into(), default_model(provider).into())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatusDto {
    recorder: crate::capture::RecorderStatus,
    analysis: AnalysisStatus,
    pending_batches: i64,
    last_capture_at: Option<i64>,
    tokens_today: (i64, i64),
    today: String,
    /// Why the last analysis failed, in plain words with its fixes (llm/failure.rs); None while
    /// the last batch went through.
    cause: Option<crate::llm::failure::cause::Cause>,
}

#[tauri::command]
pub fn get_status(state: S) -> CmdResult<StatusDto> {
    let today = logical_day(now_ts());
    let (day_start, _) = logical_day_bounds(&today).unwrap_or((now_ts() - 86400, now_ts()));
    let analysis = state.analysis.lock().unwrap().clone();
    let cause = analysis.last_error.as_deref().map(|e| {
        let s = state.settings();
        crate::llm::failure::cause::describe(e, crate::llm::failure::cause::CauseCtx { provider: s.provider, base_url: &s.base_url, recording: s.recording_enabled })
    });
    Ok(StatusDto {
        recorder: state.recorder.lock().unwrap().clone(),
        analysis,
        pending_batches: state.db.count_batches_with_status(&["pending", "processing"]).map_err(err)?,
        last_capture_at: state.db.last_capture_ts().map_err(err)?,
        tokens_today: state.db.token_usage_since(day_start).map_err(err)?,
        today,
        cause,
    })
}

#[tauri::command]
pub fn set_recording(app: tauri::AppHandle, state: S, enabled: bool) -> CmdResult<()> {
    crate::features::platform_extras::update_settings(&state, |g| g.recording_enabled = enabled).map_err(err)?;
    if enabled {
        state.capture_now.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    crate::tray::refresh(&app);
    Ok(())
}

#[tauri::command]
pub fn get_day(state: S, day: String) -> CmdResult<Vec<StoredCard>> {
    state.db.cards_for_day(&day).map_err(err)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShotDto {
    id: i64,
    captured_at: i64,
    app_name: Option<String>,
    window_title: Option<String>,
    hidden_app: Option<String>,
    skip_reason: Option<String>,
}

#[tauri::command]
pub fn get_card_screenshots(state: S, card_id: i64, max: Option<usize>) -> CmdResult<Vec<ShotDto>> {
    let card = state.db.card(card_id).map_err(err)?.ok_or("card not found")?;
    let shots: Vec<_> = state
        .db
        .screenshots_in_range(card.start_ts, card.end_ts + 60)
        .map_err(err)?
        .into_iter()
        .filter(|s| s.has_image() || s.hidden_app.is_some())
        .collect();
    let max = max.unwrap_or(24).max(1);
    // Spread over the whole card (first and last included), also when `max` is more than half of
    // the frames; a fixed step would stop at the first `max`.
    Ok(crate::llm::one_step::evenly_spaced(shots.len(), max)
        .into_iter()
        .map(|i| &shots[i])
        .map(|s| ShotDto {
            id: s.id,
            captured_at: s.captured_at,
            app_name: s.app_name.clone(),
            window_title: s.window_title.clone(),
            hidden_app: s.hidden_app.clone(),
            skip_reason: s.skip_reason.clone(),
        })
        .collect())
}

/// Returns a screenshot as a data URL. `thumb` downsizes to 360 px high.
#[tauri::command]
pub async fn get_screenshot(state: S<'_>, id: i64, thumb: bool) -> CmdResult<Option<String>> {
    let Some(shot) = state.db.screenshot(id).map_err(err)? else { return Ok(None) };
    if shot.file_path.is_empty() {
        return Ok(None);
    }
    let path = shot.file_path.clone();
    let bytes = tauri::async_runtime::spawn_blocking(move || -> anyhow::Result<Vec<u8>> {
        let raw = std::fs::read(&path)?;
        if !thumb {
            return Ok(raw);
        }
        let img = image::load_from_memory(&raw)?.to_rgb8();
        let small = crate::imaging::downscale_rgb(img, 360);
        crate::imaging::encode_jpeg(&small, 75)
    })
    .await
    .map_err(err)?;
    match bytes {
        Ok(b) => Ok(Some(format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(b)))),
        Err(_) => Ok(None), // file expired or missing
    }
}

#[tauri::command]
pub fn retry_card(state: S, card_id: i64) -> CmdResult<()> {
    let card = state.db.card(card_id).map_err(err)?.ok_or("card not found")?;
    let batch_id = card.batch_id.ok_or("card has no batch")?;
    crate::analysis::request_retry(&state, batch_id).map_err(err)
}

#[tauri::command]
pub fn analyze_now(state: S) {
    state.analyze_now.notify_one();
}

fn client_for(provider: ProviderKind, base_url: &str, model: &str, api_key: Option<String>, fallback: bool) -> CmdResult<LlmClient> {
    // A key typed in the form is used as-is; otherwise only the key stored for this exact
    // provider and host is used (the fallback provider's own key, for the fallback).
    let stored = || if fallback { crate::config::stored_fallback_key(provider, base_url) } else { crate::config::stored_key(provider, base_url) };
    let key = api_key.filter(|k| !k.trim().is_empty()).or_else(stored);
    LlmClient::new(provider, base_url, model, key, None).map_err(err)
}

#[tauri::command]
pub async fn list_models(provider: ProviderKind, base_url: String, api_key: Option<String>) -> CmdResult<Vec<String>> {
    client_for(provider, &base_url, "", api_key, false)?.list_models().await.map_err(err)
}

/// Sends a one-line prompt plus a tiny image, so vision support is checked too. `fallback`: test
/// with the fallback provider's stored key.
#[tauri::command]
pub async fn test_provider(provider: ProviderKind, base_url: String, model: String, api_key: Option<String>, fallback: Option<bool>) -> CmdResult<String> {
    let client = client_for(provider, &base_url, &model, api_key, fallback.unwrap_or(false))?;
    let img = image::RgbImage::from_pixel(32, 32, image::Rgb([30, 120, 220]));
    let jpg = crate::imaging::encode_jpeg(&img, 90).map_err(err)?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(jpg);
    let started = std::time::Instant::now();
    let out = client
        .complete(
            vec![Part::Text("What is the main color of this image? Answer in one word.".into()), Part::Jpeg { b64, w: 32, h: 32 }],
            CallOpts { op: "test", batch_id: None, max_tokens: 50, system: None, attempts: 1 },
        )
        .await
        .map_err(err)?;
    Ok(format!("{} ({} ms)", out.trim(), started.elapsed().as_millis()))
}

#[tauri::command]
pub fn export_markdown(state: S, from_day: String, to_day: String, path: String) -> CmdResult<usize> {
    let md = crate::export::markdown(&state.db, &from_day, &to_day).map_err(err)?;
    std::fs::write(&path, md.0).map_err(err)?;
    Ok(md.1)
}

#[tauri::command]
pub fn open_data_dir(app: tauri::AppHandle, state: S) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_path(state.paths.root.to_string_lossy(), None::<&str>).map_err(err)
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
    fn a_save_from_old_settings_is_refused() {
        let (_d, st) = state();
        let (mut mine, base) = st.settings_with_revision();
        // An AI tool adds a category (the bridge) after the window read the settings.
        st.settings.write().unwrap().allow_agent_edits = true;
        crate::features::agent_bridge::writes::handle(&st, "category_add", &serde_json::json!({"name": "Reading"})).unwrap();
        assert!(st.settings_revision() > base);
        // The window's save made on the old copy would remove it: refused, with the revision now.
        mine.retention_days = 3;
        let e = store_settings(&st, mine.clone(), Some(base)).unwrap_err();
        assert_eq!(e, SaveError::Stale { code: SETTINGS_CHANGED, message: "The settings changed while this change was being saved. Try again.".into(), revision: st.settings_revision() });
        assert!(st.settings().categories.iter().any(|c| c.name == "Reading"));
        assert_eq!(st.settings().retention_days, Settings::default().retention_days);
        // Re-read and applied again, it goes through and counts as a change.
        let (mut fresh, rev) = st.settings_with_revision();
        fresh.retention_days = 3;
        assert_eq!(store_settings(&st, fresh, Some(rev)).unwrap(), rev + 1);
        assert_eq!(st.settings().retention_days, 3);
        assert!(Settings::load(&st.paths.settings).categories.iter().any(|c| c.name == "Reading"));
        // Without a base revision nothing is checked (callers from before revisions).
        assert!(store_settings(&st, mine, None).is_ok());
    }

    #[test]
    fn every_saved_change_counts() {
        let (_d, st) = state();
        let r0 = st.settings_revision();
        crate::features::platform_extras::update_settings(&st, |s| s.recording_enabled = false).unwrap();
        assert_eq!(st.settings_revision(), r0 + 1, "tray, pause, platform loop");
        let (s, r1) = st.settings_with_revision();
        assert_eq!(r1, r0 + 1);
        assert!(!s.recording_enabled);
        store_settings(&st, s, Some(r1)).unwrap();
        assert_eq!(st.settings_revision(), r1 + 1, "the window's save");
    }

    #[test]
    fn errors_serialize_for_the_window() {
        let stale = SaveError::Stale { code: SETTINGS_CHANGED, message: "m".into(), revision: 7 };
        assert_eq!(serde_json::to_value(&stale).unwrap(), serde_json::json!({"code": "settings_changed", "message": "m", "revision": 7}));
        assert_eq!(serde_json::to_value(SaveError::from("plain")).unwrap(), serde_json::json!("plain"));
    }
}
