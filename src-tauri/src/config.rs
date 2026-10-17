use crate::types::{default_categories, Category};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// Native Anthropic Messages API.
    Anthropic,
    /// Ollama's OpenAI-compatible endpoint.
    Ollama,
    /// LM Studio's OpenAI-compatible endpoint.
    LmStudio,
    /// Any other OpenAI-compatible endpoint (OpenAI, OpenRouter, Gemini's /openai endpoint, ...).
    OpenaiCompatible,
    /// The Claude Code CLI (`claude -p`), signed in with the user's Claude subscription.
    /// `base_url` holds the path to the executable; empty means auto-detect.
    ClaudeCli,
    /// The Codex CLI (`codex exec`), signed in with the user's ChatGPT account.
    /// `base_url` holds the path to the executable; empty means auto-detect.
    CodexCli,
}

impl ProviderKind {
    /// Providers that run a local command-line tool instead of calling an HTTP endpoint.
    pub fn is_cli(self) -> bool {
        matches!(self, ProviderKind::ClaudeCli | ProviderKind::CodexCli)
    }

    /// Short name for messages ("Claude Code CLI could not ...").
    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::Anthropic => "Claude API",
            ProviderKind::Ollama => "Ollama",
            ProviderKind::LmStudio => "LM Studio",
            ProviderKind::OpenaiCompatible => "OpenAI-compatible API",
            ProviderKind::ClaudeCli => "Claude Code CLI",
            ProviderKind::CodexCli => "Codex CLI",
        }
    }

    /// The analysis mode that suits the provider when it is used as the fallback.
    pub fn natural_pipeline(self) -> Pipeline {
        match self {
            ProviderKind::Ollama | ProviderKind::LmStudio => Pipeline::Local,
            _ => Pipeline::Cloud,
        }
    }
}

/// User edits of the instruction blocks inside the card prompts (Dayflow's
/// ActivityCardPromptOverrides / OllamaPromptOverrides). Empty means the default text.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct PromptOverrides {
    /// Title instructions of the Detailed mode card prompt.
    pub detailed_title: String,
    /// Summary instructions of the Detailed mode card prompt.
    pub detailed_summary: String,
    /// Title instructions of the Lightweight mode title prompt.
    pub lightweight_title: String,
    /// Summary guidelines of the Lightweight mode summary prompt.
    pub lightweight_summary: String,
}

impl PromptOverrides {
    /// Dayflow's compose rule: a trimmed, non-empty override replaces the default block.
    pub fn pick<'a>(custom: &'a str, default: &'a str) -> &'a str {
        let t = custom.trim();
        if t.is_empty() { default } else { t }
    }
}

/// How much hidden reasoning Claude may spend. Haiku 5.5 thinks by default (effort "medium"),
/// and thinking tokens are billed as output: in real use billed output was 2.4-2.8x the visible
/// text. Dayflow's prompts already make the model reason in the open (titleEvidence).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Reasoning {
    /// thinking disabled, effort low
    Off,
    /// adaptive thinking, effort low
    Low,
    /// the model's default
    Default,
}

/// Which of Dayflow's two analysis pipelines to run.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Pipeline {
    /// Small local models: one caption per frame, then segment, summary, title, merge.
    Local,
    /// Capable models: all frames in one request, then a rewrite of the last 45 minutes of cards.
    Cloud,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub recording_enabled: bool,
    pub capture_interval_secs: u64,
    pub capture_height: u32,
    pub jpeg_quality: u8,
    pub retention_days: u32,
    pub provider: ProviderKind,
    pub base_url: String,
    pub model: String,
    pub pipeline: Pipeline,
    pub categories: Vec<Category>,
    pub blocked_apps: Vec<String>,
    /// Output language for summaries; empty means the model's default (English).
    pub output_language: String,
    pub launch_at_login: bool,
    /// Stop storing screenshots after this many seconds without keyboard or mouse input, unless
    /// an app keeps the display on (video). 0 disables the guard.
    pub idle_pause_secs: u64,
    /// Send Claude requests through the Message Batches API (50% cheaper, results can take
    /// minutes to an hour). Anthropic provider only.
    pub use_batch_api: bool,
    /// Hidden reasoning for Claude models (Anthropic provider only).
    pub reasoning: Reasoning,
    /// Folder for the daily recap Markdown files (`<dir>/Daytrace/<day>.md`); empty turns the
    /// export off.
    pub recap_export_dir: String,
    /// Write the daily recap automatically after `recap_time` (and export it when a folder is set).
    /// Opt-in: it spends a model request a day.
    pub recap_auto: bool,
    /// Local time ("HH:MM", 24-hour) after which the automatic recap runs.
    pub recap_time: String,
    /// Model for chat, on the same provider; empty uses `model`.
    pub chat_model: String,
    /// Second provider that takes over a step when the main one fails (Dayflow's secondary
    /// provider in LLMProviderRouting). None turns the fallback off.
    pub fallback_provider: Option<ProviderKind>,
    /// Server address of the fallback provider (CLI path for the CLI providers).
    pub fallback_base_url: String,
    /// Model of the fallback provider.
    pub fallback_model: String,
    /// Edited instruction blocks of the card prompts.
    pub prompt_overrides: PromptOverrides,
    // ---- platform_extras ----
    /// First-run setup was finished or skipped. Recording waits for it.
    pub onboarding_completed: bool,
    /// End of a timed pause (Unix seconds). Set only while `recording_enabled` is false; recording
    /// resumes at this time. None with recording off means paused until resumed.
    pub pause_until: Option<i64>,
    /// The three timed pause lengths in minutes (Dayflow's PausePresets, default 15, 30, 60).
    pub pause_presets: Vec<u32>,
    /// Light/dark override for the window.
    pub appearance: crate::features::platform_extras::Appearance,
    /// Which notifications Daytrace may show.
    pub notifications: crate::features::platform_extras::NotificationPrefs,
    /// Look for a new release on GitHub once a day.
    pub check_updates: bool,
    /// Maximum disk space for screenshots in GB (10^9 bytes); the oldest are deleted first when
    /// they take more. 0 turns the limit off (retention_days still applies).
    pub storage_limit_gb: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            recording_enabled: true,
            capture_interval_secs: 10,
            capture_height: 720,
            jpeg_quality: 70,
            retention_days: 7,
            provider: ProviderKind::Anthropic,
            base_url: default_base_url(ProviderKind::Anthropic).into(),
            model: default_model(ProviderKind::Anthropic).into(),
            pipeline: Pipeline::Cloud,
            categories: default_categories(),
            blocked_apps: crate::platform::DEFAULT_BLOCKED_APPS.iter().map(|s| s.to_string()).collect(),
            output_language: String::new(),
            launch_at_login: false,
            idle_pause_secs: 120,
            use_batch_api: false,
            reasoning: Reasoning::Low,
            recap_export_dir: String::new(),
            recap_auto: false,
            recap_time: "08:00".into(),
            chat_model: String::new(),
            fallback_provider: None,
            fallback_base_url: String::new(),
            fallback_model: String::new(),
            prompt_overrides: PromptOverrides::default(),
            onboarding_completed: false,
            pause_until: None,
            pause_presets: crate::features::platform_extras::pause::DEFAULT_PRESETS.to_vec(),
            appearance: Default::default(),
            notifications: Default::default(),
            check_updates: true,
            storage_limit_gb: 0,
        }
    }
}

pub fn default_base_url(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "https://api.anthropic.com",
        ProviderKind::Ollama => "http://localhost:11434",
        ProviderKind::LmStudio => "http://localhost:1234",
        ProviderKind::OpenaiCompatible => "https://api.openai.com/v1",
        // Empty: find the executable automatically.
        ProviderKind::ClaudeCli | ProviderKind::CodexCli => "",
    }
}

/// Credential slot for a provider. Keys are bound to the provider and, for generic endpoints, to
/// the host, so a stored key is never sent to a different server. Ollama never gets a key.
pub fn key_slot(kind: ProviderKind, base_url: &str) -> Option<String> {
    match kind {
        ProviderKind::Anthropic => Some("anthropic".into()),
        ProviderKind::LmStudio => Some("lm_studio".into()),
        // Ollama needs no key; the CLIs use their own sign-in.
        ProviderKind::Ollama | ProviderKind::ClaudeCli | ProviderKind::CodexCli => None,
        ProviderKind::OpenaiCompatible => {
            let host = reqwest::Url::parse(base_url.trim()).ok()?.host_str()?.to_lowercase();
            Some(format!("openai:{host}"))
        }
    }
}

/// The stored key for this provider and server, if any.
pub fn stored_key(kind: ProviderKind, base_url: &str) -> Option<String> {
    key_slot(kind, base_url).and_then(|s| crate::platform::secret_get(&s))
}

pub fn default_model(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "claude-haiku-5-5",
        ProviderKind::Ollama => "qwen3-vl:4b",
        ProviderKind::LmStudio => "qwen3-vl-4b-instruct",
        ProviderKind::OpenaiCompatible => "gpt-5-mini",
        // Dayflow's one-step timeline models (ClaudeProvider+Timeline.swift,
        // CodexProvider+Timeline.swift).
        ProviderKind::ClaudeCli => "haiku",
        ProviderKind::CodexCli => "gpt-6-luna",
    }
}

impl Settings {
    pub fn load(path: &Path) -> Settings {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str::<Settings>(&s).ok())
            .map(|s| s.sanitized())
            .unwrap_or_default()
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn sanitized(mut self) -> Settings {
        self.capture_interval_secs = self.capture_interval_secs.clamp(1, 60);
        self.capture_height = if self.capture_height <= 720 { 720 } else { 1080 };
        self.jpeg_quality = self.jpeg_quality.clamp(30, 95);
        self.retention_days = self.retention_days.clamp(1, 365);
        self.storage_limit_gb = self.storage_limit_gb.min(1000);
        if self.idle_pause_secs != 0 {
            self.idle_pause_secs = self.idle_pause_secs.clamp(30, 3600);
        }
        if self.categories.iter().all(|c| c.name.trim().is_empty()) {
            self.categories = default_categories();
        }
        self.categories.retain(|c| !c.name.trim().is_empty());
        if !self.categories.iter().any(|c| c.is_idle) {
            self.categories.push(Category {
                name: "Idle".into(),
                color: "#A0AEC0".into(),
                description: String::new(),
                is_idle: true,
            });
        }
        if self.base_url.trim().is_empty() {
            self.base_url = default_base_url(self.provider).into();
        }
        if self.model.trim().is_empty() {
            self.model = default_model(self.provider).into();
        }
        // A relative recap folder would resolve against the working directory (save refuses it).
        self.recap_export_dir = self.recap_export_dir.trim().to_string();
        if !self.recap_export_dir.is_empty() && !Path::new(&self.recap_export_dir).is_absolute() {
            self.recap_export_dir.clear();
        }
        if let Some(kind) = self.fallback_provider {
            if self.fallback_base_url.trim().is_empty() {
                self.fallback_base_url = default_base_url(kind).into();
            }
            if self.fallback_model.trim().is_empty() {
                self.fallback_model = default_model(kind).into();
            }
            // Dayflow drops a secondary that equals the primary.
            if kind == self.provider
                && self.fallback_base_url.trim().trim_end_matches('/') == self.base_url.trim().trim_end_matches('/')
                && self.fallback_model.trim() == self.model.trim()
            {
                self.fallback_provider = None;
            }
        }
        self
    }
}

pub struct Paths {
    pub root: PathBuf,
    pub db: PathBuf,
    pub settings: PathBuf,
    pub screenshots: PathBuf,
}

impl Paths {
    pub fn new(root: PathBuf) -> anyhow::Result<Paths> {
        let screenshots = root.join("screenshots");
        std::fs::create_dir_all(&screenshots)?;
        Ok(Paths { db: root.join("daytrace.sqlite"), settings: root.join("settings.json"), screenshots, root })
    }
}
