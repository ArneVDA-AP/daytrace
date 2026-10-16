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
}

#[tauri::command]
pub fn get_settings(state: S) -> SettingsDto {
    SettingsDto {
        settings: state.settings(),
        has_api_key: crate::platform::secret_get().is_some(),
        data_dir: state.paths.root.to_string_lossy().to_string(),
    }
}

#[tauri::command]
pub fn save_settings(app: tauri::AppHandle, state: S, settings: Settings, api_key: Option<String>) -> CmdResult<SettingsDto> {
    let settings = settings.sanitized();
    settings.save(&state.paths.settings).map_err(err)?;
    if let Some(k) = api_key {
        crate::platform::secret_set(k.trim()).map_err(err)?;
    }
    {
        use tauri_plugin_autostart::ManagerExt;
        let al = app.autolaunch();
        let _ = if settings.launch_at_login { al.enable() } else { al.disable() };
    }
    *state.settings.write().unwrap() = settings;
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
}

#[tauri::command]
pub fn get_status(state: S) -> CmdResult<StatusDto> {
    let today = logical_day(now_ts());
    let (day_start, _) = logical_day_bounds(&today).unwrap_or((now_ts() - 86400, now_ts()));
    Ok(StatusDto {
        recorder: state.recorder.lock().unwrap().clone(),
        analysis: state.analysis.lock().unwrap().clone(),
        pending_batches: state.db.count_batches_with_status(&["pending", "processing"]).map_err(err)?,
        last_capture_at: state.db.last_capture_ts().map_err(err)?,
        tokens_today: state.db.token_usage_since(day_start).map_err(err)?,
        today,
    })
}

#[tauri::command]
pub fn set_recording(app: tauri::AppHandle, state: S, enabled: bool) -> CmdResult<()> {
    let s = {
        let mut g = state.settings.write().unwrap();
        g.recording_enabled = enabled;
        g.clone()
    };
    s.save(&state.paths.settings).map_err(err)?;
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
}

#[tauri::command]
pub fn get_card_screenshots(state: S, card_id: i64, max: Option<usize>) -> CmdResult<Vec<ShotDto>> {
    let card = state.db.card(card_id).map_err(err)?.ok_or("card not found")?;
    let shots = state.db.screenshots_in_range(card.start_ts, card.end_ts + 60).map_err(err)?;
    let max = max.unwrap_or(24).max(1);
    let step = (shots.len() / max).max(1);
    Ok(shots
        .into_iter()
        .step_by(step)
        .take(max)
        .map(|s| ShotDto { id: s.id, captured_at: s.captured_at, app_name: s.app_name, window_title: s.window_title, hidden_app: s.hidden_app })
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
        let img = image::load_from_memory(&raw)?;
        let small = img.resize(10_000, 360, image::imageops::FilterType::Triangle).to_rgb8();
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 75).encode_image(&small)?;
        Ok(out)
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

fn client_for(provider: ProviderKind, base_url: &str, model: &str, api_key: Option<String>) -> CmdResult<LlmClient> {
    let key = api_key.filter(|k| !k.trim().is_empty()).or_else(crate::platform::secret_get);
    LlmClient::new(provider, base_url, model, key, None).map_err(err)
}

#[tauri::command]
pub async fn list_models(provider: ProviderKind, base_url: String, api_key: Option<String>) -> CmdResult<Vec<String>> {
    client_for(provider, &base_url, "", api_key)?.list_models().await.map_err(err)
}

/// Sends a one-line prompt plus a tiny image, so vision support is checked too.
#[tauri::command]
pub async fn test_provider(provider: ProviderKind, base_url: String, model: String, api_key: Option<String>) -> CmdResult<String> {
    let client = client_for(provider, &base_url, &model, api_key)?;
    let mut png = Vec::new();
    let img = image::RgbImage::from_pixel(32, 32, image::Rgb([30, 120, 220]));
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut png, 90).encode_image(&img).map_err(err)?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    let started = std::time::Instant::now();
    let out = client
        .complete(
            vec![Part::Text("What is the main color of this image? Answer in one word.".into()), Part::Jpeg(b64)],
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
