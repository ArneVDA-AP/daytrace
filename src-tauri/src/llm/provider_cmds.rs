//! Tauri commands for the provider settings: the fallback provider's key, the CLI check, and the
//! default prompt blocks shown in the prompt editor.

use crate::config::{fallback_key_slot, stored_fallback_key, PromptOverrides, ProviderKind};
use serde::Serialize;

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

/// The default instruction blocks, to show next to the user's edits.
#[tauri::command]
pub fn provider_prompt_defaults() -> PromptOverrides {
    super::prompts::default_blocks()
}

/// Whether the fallback provider has a key for this provider and server: its own, or the one
/// it shares with the main provider (see `config::stored_fallback_key`).
#[tauri::command]
pub fn provider_key_status(provider: ProviderKind, base_url: String) -> bool {
    stored_fallback_key(provider, &base_url).is_some()
}

/// Stores the fallback provider's key in its own slot, so it never replaces the main provider's
/// key (that one is saved with the settings), also when both use the same provider.
#[tauri::command]
pub fn provider_save_key(provider: ProviderKind, base_url: String, api_key: String) -> Result<bool, String> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err("The key is empty.".into());
    }
    let slot = fallback_key_slot(provider, &base_url).ok_or("This provider does not use an API key")?;
    crate::platform::secret_set(&slot, key).map_err(err)?;
    Ok(true)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliCheck {
    pub path: String,
    pub version: String,
}

/// Finds the CLI (Settings path or automatic search) and asks it for its version. Runs only
/// `--version`; it does not contact the model.
#[tauri::command]
pub async fn provider_check_cli(provider: ProviderKind, path: String) -> Result<CliCheck, String> {
    let tool = super::cli::CliTool::from_kind(provider).ok_or("This provider is not a command-line tool")?;
    let backend = super::cli::CliBackend::new(tool, &path, "");
    let (exe, version) = backend.version().await.map_err(err)?;
    Ok(CliCheck { path: exe.display().to_string(), version })
}
