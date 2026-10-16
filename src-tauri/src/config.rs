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
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            recording_enabled: true,
            capture_interval_secs: 10,
            capture_height: 1080,
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
        }
    }
}

pub fn default_base_url(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "https://api.anthropic.com",
        ProviderKind::Ollama => "http://localhost:11434",
        ProviderKind::LmStudio => "http://localhost:1234",
        ProviderKind::OpenaiCompatible => "https://api.openai.com/v1",
    }
}

pub fn default_model(kind: ProviderKind) -> &'static str {
    match kind {
        ProviderKind::Anthropic => "claude-haiku-5-5",
        ProviderKind::Ollama => "qwen3-vl:4b",
        ProviderKind::LmStudio => "qwen3-vl-4b-instruct",
        ProviderKind::OpenaiCompatible => "gpt-5-mini",
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
