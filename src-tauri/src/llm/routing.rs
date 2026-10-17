//! Main and fallback provider (Dayflow LLMProviderRouting.swift and
//! LLMService.executeWithProviderBackup): each analysis step runs on the active provider; when it
//! fails while the main provider is active, it runs once more on the fallback, which then stays
//! active for the rest of the batch. A main provider that cannot start (Claude API without a
//! key) hands the whole batch to the fallback.

use super::client::LlmClient;
use crate::config::{stored_key, Pipeline, ProviderKind, Settings};
use crate::db::Db;
use anyhow::{anyhow, Result};
use std::future::Future;

pub struct Lane {
    pub client: LlmClient,
    pub pipeline: Pipeline,
    /// False for the fallback provider.
    pub is_primary: bool,
}

/// A provider that cannot run at all: the Claude API without a stored key.
fn usable(kind: ProviderKind, base_url: &str) -> bool {
    kind != ProviderKind::Anthropic || stored_key(kind, base_url).is_some()
}

pub fn has_usable_fallback(s: &Settings) -> bool {
    s.fallback_provider.map(|k| usable(k, &s.fallback_base_url)).unwrap_or(false)
}

fn fallback_lane(s: &Settings, db: Option<Db>) -> Option<Lane> {
    let kind = s.fallback_provider?;
    if !usable(kind, &s.fallback_base_url) {
        log::warn!("fallback provider {} has no API key; not used", kind.label());
        return None;
    }
    let key = stored_key(kind, &s.fallback_base_url);
    let mut client = LlmClient::new(kind, &s.fallback_base_url, &s.fallback_model, key, db).ok()?;
    // The fallback answers now: no Batch API.
    client.reasoning = s.reasoning;
    Some(Lane { client, pipeline: kind.natural_pipeline(), is_primary: false })
}

/// The providers to use for one batch, in order: [main, fallback] or [fallback] alone when the
/// main provider cannot start.
pub fn lanes(s: &Settings, db: Option<Db>) -> Result<Vec<Lane>> {
    let fallback = fallback_lane(s, db.clone());
    if !usable(s.provider, &s.base_url) {
        return fallback.map(|f| vec![f]).ok_or_else(|| anyhow!("No Anthropic API key configured"));
    }
    let key = stored_key(s.provider, &s.base_url);
    let mut client = LlmClient::new(s.provider, &s.base_url, &s.model, key, db)?;
    client.use_batch = s.use_batch_api && s.provider == ProviderKind::Anthropic;
    client.reasoning = s.reasoning;
    let mut out = vec![Lane { client, pipeline: s.pipeline, is_primary: true }];
    out.extend(fallback);
    Ok(out)
}

/// Runs one step on lane `*active`; on failure while lane 0 is active and a second lane exists,
/// runs it once on lane 1 and keeps lane 1 active. `work` gets the lane index.
pub async fn run_stage<T, F, Fut>(stage: &str, lane_count: usize, active: &mut usize, mut work: F) -> Result<T>
where
    F: FnMut(usize) -> Fut,
    Fut: Future<Output = Result<T>>,
{
    match work(*active).await {
        Ok(v) => Ok(v),
        Err(e) if *active == 0 && lane_count > 1 => {
            log::warn!("{stage} failed on the main provider ({e:#}); trying the fallback provider");
            *active = 1;
            work(1).await.map_err(|e2| anyhow!("{e2:#}\n\nThe main provider failed first: {e:#}"))
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[tokio::test]
    async fn fallback_takes_over_and_stays() {
        let calls = Mutex::new(Vec::new());
        let mut active = 0;
        // Main provider fails the first step: the fallback runs it.
        let r: Result<&str> = run_stage("transcribe", 2, &mut active, |i| {
            calls.lock().unwrap().push(("transcribe", i));
            async move { if i == 0 { Err(anyhow!("HTTP 529: overloaded")) } else { Ok("observations") } }
        })
        .await;
        assert_eq!(r.unwrap(), "observations");
        assert_eq!(active, 1);
        // The next step goes straight to the fallback.
        let r: Result<&str> = run_stage("generate_cards", 2, &mut active, |i| {
            calls.lock().unwrap().push(("generate_cards", i));
            async move { Ok(if i == 1 { "cards from fallback" } else { "wrong" }) }
        })
        .await;
        assert_eq!(r.unwrap(), "cards from fallback");
        assert_eq!(*calls.lock().unwrap(), vec![("transcribe", 0), ("transcribe", 1), ("generate_cards", 1)]);
    }

    #[tokio::test]
    async fn no_fallback_or_both_failing() {
        let mut active = 0;
        let r: Result<()> = run_stage("transcribe", 1, &mut active, |_| async { Err(anyhow!("primary down")) }).await;
        assert_eq!(r.unwrap_err().to_string(), "primary down");
        assert_eq!(active, 0);

        let mut active = 0;
        let r: Result<()> = run_stage("transcribe", 2, &mut active, |i| async move {
            Err(anyhow!(if i == 0 { "Claude Code CLI: Not logged in · Please run /login" } else { "connection refused" }))
        })
        .await;
        let msg = r.unwrap_err().to_string();
        assert!(msg.starts_with("connection refused"));
        assert!(msg.contains("The main provider failed first: Claude Code CLI: Not logged in"));
        // Once on the fallback, a failure is final (no bounce back to the main provider).
        let mut calls = 0;
        let r: Result<()> = run_stage("generate_cards", 2, &mut active, |_| {
            calls += 1;
            async { Err(anyhow!("still down")) }
        })
        .await;
        assert!(r.is_err());
        assert_eq!(calls, 1);
    }

    #[test]
    fn lanes_follow_settings() {
        let mut s = Settings { provider: ProviderKind::ClaudeCli, base_url: String::new(), model: "haiku".into(), ..Default::default() };
        let l = lanes(&s, None).unwrap();
        assert_eq!(l.len(), 1);
        assert!(l[0].client.is_cli() && l[0].is_primary);

        s.fallback_provider = Some(ProviderKind::Ollama);
        s = s.sanitized();
        assert_eq!(s.fallback_base_url, "http://localhost:11434");
        assert_eq!(s.fallback_model, "qwen3-vl:4b");
        let l = lanes(&s, None).unwrap();
        assert_eq!(l.len(), 2);
        assert_eq!(l[1].pipeline, Pipeline::Local);
        assert!(!l[1].is_primary);
        assert_eq!(l[1].client.kind, ProviderKind::Ollama);

        // A fallback equal to the main provider is dropped.
        let same = Settings { fallback_provider: Some(ProviderKind::ClaudeCli), fallback_base_url: String::new(), fallback_model: "haiku".into(), ..s.clone() }.sanitized();
        assert_eq!(same.fallback_provider, None);
    }
}
