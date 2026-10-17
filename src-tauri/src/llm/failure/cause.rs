//! The last analysis failure as the rail and the global banner show it (UX2): the cause in plain
//! words, what Daytrace does by itself and the fixes the banner offers. `get_status` sends it as
//! `cause`, so the UI never sorts error texts itself.
//!
//! What Daytrace does by itself (analysis.rs): every new batch of screenshots (about every 15
//! minutes while recording) is analyzed as it comes; once one succeeds, failed batches of the last
//! 24 hours are tried again, up to 3 times each. So nothing is lost while a problem lasts, but the
//! failed cards are only rewritten after a request gets through.

use super::{classify, FailureKind};
use crate::config::ProviderKind;
use serde::Serialize;

/// A button in the failure banner; the first one is the main fix.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Fix {
    /// Settings → Model provider.
    Settings { label: &'static str },
    /// One of the named pages `shell_open_link` opens.
    Link { label: &'static str, target: &'static str },
    /// Retry today's failed batches now (or analyze the queue).
    Retry { label: &'static str },
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Cause {
    pub kind: FailureKind,
    /// Provider named by the error text itself ("Claude", "ChatGPT", ...).
    pub provider: Option<&'static str>,
    /// The provider could not be reached (network, DNS, a local server that is not running): the
    /// UI shows its "Can’t reach …" banner instead of the failure banner.
    pub unreachable: bool,
    /// Short line for the rail ("Claude refused the API key").
    pub label: String,
    /// The cause and what Daytrace does by itself, in a few plain sentences (banner body).
    pub body: String,
    /// Second line in the rail: what happens next.
    pub next: String,
    pub fixes: Vec<Fix>,
    /// Daytrace gets past this by itself (busy servers, an unreadable answer).
    pub self_healing: bool,
}

/// What the wording depends on besides the error text.
#[derive(Debug, Clone, Copy)]
pub struct CauseCtx<'a> {
    pub provider: ProviderKind,
    pub base_url: &'a str,
    /// Recording is on (new batches keep coming), so the text can promise the next try.
    pub recording: bool,
}

/// Words in network failures: these go to the "Can’t reach" banner, the rest of the transient
/// ones are a busy server.
const NETWORK: &[&str] = &[
    "error sending request", "could not reach", "could not connect", "unable to connect", "connection", "connect error", "timed out",
    "timeout", "dns", "resolve", "unreachable", "network", "no route", "refused", "offline", "failed to fetch", "econnreset",
    "enotfound", "socket", "hostname", "tls error", "ssl error",
];

fn is_cli(p: ProviderKind) -> bool {
    matches!(p, ProviderKind::ClaudeCli | ProviderKind::CodexCli)
}

/// The provider could not be reached (network, DNS, a local server that is not running). A CLI
/// provider that times out is slow, not unreachable: that one gets the failure banner.
pub fn is_unreachable(message: &str, provider: ProviderKind) -> bool {
    let kind = classify(message).kind;
    if kind == FailureKind::LocalEngineOffline {
        return true;
    }
    if is_cli(provider) {
        return false;
    }
    let lower = message.to_lowercase();
    matches!(kind, FailureKind::Transient | FailureKind::Unknown) && NETWORK.iter().any(|w| lower.contains(w))
}

/// "Claude", "Claude Code", "Ollama", ... for the configured provider; the host (with its port)
/// for another OpenAI-compatible endpoint.
pub fn configured_name(provider: ProviderKind, base_url: &str) -> String {
    match provider {
        ProviderKind::Anthropic => "Claude".into(),
        ProviderKind::ClaudeCli => "Claude Code".into(),
        ProviderKind::CodexCli => "Codex".into(),
        ProviderKind::Ollama => "Ollama".into(),
        ProviderKind::LmStudio => "LM Studio".into(),
        ProviderKind::OpenaiCompatible => host(base_url).unwrap_or_else(|| "your AI provider".into()),
    }
}

/// `host[:port]` of a URL, the port only when it is not the scheme's default (like URL.host).
fn host(url: &str) -> Option<String> {
    let u = reqwest::Url::parse(url.trim()).ok()?;
    let h = u.host_str().filter(|h| !h.is_empty())?;
    Some(match u.port() {
        Some(p) => format!("{h}:{p}"),
        None => h.to_string(),
    })
}

const RETRY: Fix = Fix::Retry { label: "Try again" };
const MODEL: Fix = Fix::Settings { label: "Provider settings" };

/// The failure of the last batch as the rail and the banner show it.
pub fn describe(message: &str, ctx: CauseCtx<'_>) -> Cause {
    let c = classify(message);
    let named = c.provider.filter(|p| *p != "Ollama/LM Studio");
    let configured = configured_name(ctx.provider, ctx.base_url);
    // The CLI providers' error texts name the service ("Claude"); the copy names the tool.
    let who: String = if is_cli(ctx.provider) { configured.clone() } else { named.map(str::to_string).unwrap_or_else(|| configured.clone()) };
    let next_batch = if ctx.recording { "with the next batch of screenshots (about every 15 minutes)" } else { "once recording resumes and the next batch is ready" };
    let after_fix = "Screenshots are kept: once it works again, Daytrace analyzes the next batch and then rewrites the failed cards by itself.";
    let self_heal = format!("Daytrace tries again by itself {next_batch} and rewrites the failed cards once a request gets through.");
    let anthropic = ctx.provider == ProviderKind::Anthropic;

    let view = |label: String, body: String, fixes: Vec<Fix>, self_healing: bool| Cause {
        kind: c.kind,
        provider: c.provider,
        unreachable: is_unreachable(message, ctx.provider),
        label,
        body,
        next: if !self_healing {
            "Cards wait until this is fixed".into()
        } else if ctx.recording {
            "Tries again with the next batch".into()
        } else {
            "Tries again when recording resumes".into()
        },
        fixes,
        self_healing,
    };

    match c.kind {
        FailureKind::ApiKeyProblem => {
            let mut fixes = vec![Fix::Settings { label: "Replace API key" }];
            if anthropic {
                fixes.push(Fix::Link { label: "Open Claude Console", target: "console_keys" });
            }
            view(format!("{who} refused the API key"), format!("It may be revoked, expired or mistyped. Add a new key under Settings, Model provider. {after_fix}"), fixes, false)
        }
        FailureKind::OutOfCredits => view(
            format!("Your {who} account is out of credits"),
            format!("{}, or switch providers under Settings, Model provider. {after_fix}", if anthropic { "Top it up in the Claude Console" } else { "Top it up" }),
            if anthropic { vec![Fix::Link { label: "Open Claude Console", target: "console_billing" }, MODEL] } else { vec![MODEL] },
            false,
        ),
        FailureKind::UsageLimitHit => view(
            format!("{who} usage limit reached"),
            "Daytrace catches up by itself once the limit resets. To keep cards coming meanwhile, set a fallback provider under Settings, Model provider.".into(),
            vec![MODEL],
            false,
        ),
        FailureKind::ProviderLoginExpired => {
            let body = if who == "Codex" || named == Some("ChatGPT") {
                format!("Run “codex login” in PowerShell to sign in again, then choose Try again. {after_fix}")
            } else if named == Some("AWS") {
                format!("Run “aws sso login” in PowerShell, then choose Try again. {after_fix}")
            } else {
                format!("Open PowerShell, run “claude”, type “/login” to sign in again, then choose Try again. {after_fix}")
            };
            view(format!("{who} is signed out"), body, vec![RETRY, MODEL], false)
        }
        FailureKind::CliNotInstalled => {
            let git = if ctx.provider == ProviderKind::ClaudeCli { " Claude Code on Windows also needs Git for Windows." } else { "" };
            view(format!("{who} not found"), format!("Install it, or set its path under Settings, Model provider.{git} {after_fix}"), vec![MODEL], false)
        }
        FailureKind::CliOutdated => view(
            "Codex needs an update".into(),
            format!("Run “npm install -g @openai/codex@latest” in PowerShell, then choose Try again. {after_fix}"),
            vec![RETRY, MODEL],
            false,
        ),
        FailureKind::LocalEngineOffline => {
            view(format!("Can’t reach {configured}"), format!("Start it on this PC, then choose Try again. {after_fix}"), vec![RETRY, MODEL], false)
        }
        FailureKind::LocalModelMissing => view(
            "The local model isn’t loaded".into(),
            format!("Open {configured} and load the model you picked in Daytrace, then choose Try again. {after_fix}"),
            vec![RETRY, MODEL],
            false,
        ),
        FailureKind::ModelMisconfigured => view(
            "The model isn’t available".into(),
            format!("Your {who} account can’t use the model Daytrace is set to. Pick another one under Settings, Model provider. {after_fix}"),
            vec![Fix::Settings { label: "Choose a model" }],
            false,
        ),
        FailureKind::GeoBlocked => view(
            format!("{who} isn’t available here"),
            format!("It isn’t offered in your region. Switch to another provider under Settings, Model provider. {after_fix}"),
            vec![MODEL],
            false,
        ),
        FailureKind::AccountBlocked => view(
            format!("{who} blocked this account"),
            format!("A new API key or another provider under Settings, Model provider usually fixes it. {after_fix}"),
            vec![MODEL],
            false,
        ),
        FailureKind::NotConfigured => {
            view("No AI provider set up".into(), format!("Set one up under Settings, Model provider. {after_fix}"), vec![Fix::Settings { label: "Set up a provider" }], false)
        }
        FailureKind::Transient => {
            let lower = message.to_lowercase();
            if lower.contains("timed out") || lower.contains("timeout") {
                view(format!("{who} took too long to answer"), self_heal, vec![RETRY], true)
            } else {
                view(format!("{who} is busy"), format!("Its servers are overloaded or the rate limit was reached. {self_heal}"), vec![RETRY], true)
            }
        }
        FailureKind::ModelFlaky => view("The model’s answer couldn’t be read".into(), format!("This usually sorts itself out. {self_heal}"), vec![RETRY], true),
        FailureKind::Unknown => view("The last batch failed".into(), self_heal, vec![RETRY], true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = r#"HTTP 401: {"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#;

    fn ctx(provider: ProviderKind, recording: bool) -> CauseCtx<'static> {
        CauseCtx { provider, base_url: "https://api.anthropic.com", recording }
    }

    fn labels(c: &Cause) -> Vec<&'static str> {
        c.fixes
            .iter()
            .map(|f| match f {
                Fix::Settings { label } | Fix::Link { label, .. } | Fix::Retry { label } => *label,
            })
            .collect()
    }

    #[test]
    fn network_failures_go_to_the_cant_reach_banner() {
        use ProviderKind::*;
        assert!(is_unreachable("error sending request for url (https://api.anthropic.com/v1/messages): operation timed out", Anthropic));
        assert!(is_unreachable("Could not reach the model server: error sending request", Ollama));
        assert!(!is_unreachable("HTTP 429: rate limit exceeded", Anthropic), "busy is not unreachable");
        assert!(!is_unreachable(KEY, Anthropic));
        assert!(!is_unreachable("CLI process timed out after 300 seconds", ClaudeCli), "a slow CLI is not unreachable");
        assert!(describe("Could not reach the model server: error sending request", ctx(Ollama, true)).unreachable);
    }

    #[test]
    fn each_cause_has_its_own_fix() {
        let key = describe(KEY, ctx(ProviderKind::Anthropic, true));
        assert_eq!(key.kind, FailureKind::ApiKeyProblem);
        assert_eq!(key.label, "Claude refused the API key");
        assert_eq!(labels(&key), ["Replace API key", "Open Claude Console"]);
        assert_eq!(key.fixes[1], Fix::Link { label: "Open Claude Console", target: "console_keys" });
        assert!(!key.self_healing);
        assert_eq!(key.next, "Cards wait until this is fixed");

        let credits = describe("HTTP 400: Your credit balance is too low", ctx(ProviderKind::Anthropic, true));
        assert_eq!(credits.fixes[0], Fix::Link { label: "Open Claude Console", target: "console_billing" });
        let elsewhere = describe("HTTP 400: Your credit balance is too low", CauseCtx { provider: ProviderKind::OpenaiCompatible, base_url: "https://openrouter.ai/api/v1", recording: true });
        assert_eq!(elsewhere.label, "Your openrouter.ai account is out of credits");
        assert_eq!(labels(&elsewhere), ["Provider settings"]);

        let busy = describe("HTTP 429: rate limit exceeded", ctx(ProviderKind::Anthropic, true));
        assert_eq!(busy.label, "Claude is busy");
        assert!(busy.self_healing);
        assert_eq!(labels(&busy), ["Try again"]);
        assert!(busy.body.contains("tries again by itself with the next batch"), "{}", busy.body);

        let flaky = describe("The model's answer could not be read: failed to parse", ctx(ProviderKind::Anthropic, false));
        assert_eq!(flaky.kind, FailureKind::ModelFlaky);
        assert!(flaky.body.contains("once recording resumes"));
        assert_eq!(flaky.next, "Tries again when recording resumes");
    }

    #[test]
    fn cli_providers_are_named_by_the_tool() {
        let out = describe("Claude Code CLI: Not logged in · Please run /login", ctx(ProviderKind::ClaudeCli, true));
        assert_eq!(out.label, "Claude Code is signed out");
        assert!(out.body.contains("run “claude”, type “/login”"));
        assert_eq!(out.provider, Some("Claude"));
        let slow = describe("CLI process timed out after 300 seconds", ctx(ProviderKind::ClaudeCli, true));
        assert_eq!(slow.label, "Claude Code took too long to answer");
        assert!(!slow.unreachable);
        let codex = describe("Codex CLI: ERROR: You've hit your usage limit.", ctx(ProviderKind::CodexCli, true));
        assert_eq!(codex.label, "Codex usage limit reached");
        let missing = describe("Claude Code CLI not found. Install it", ctx(ProviderKind::ClaudeCli, true));
        assert!(missing.body.contains("Git for Windows"));
    }

    #[test]
    fn local_providers_name_the_app() {
        let c = describe("Could not reach the model server: error sending request", CauseCtx { provider: ProviderKind::LmStudio, base_url: "http://localhost:1234/v1", recording: true });
        assert_eq!(c.label, "Can’t reach LM Studio");
        assert_eq!(configured_name(ProviderKind::OpenaiCompatible, "http://10.0.0.5:8080/v1"), "10.0.0.5:8080");
        assert_eq!(configured_name(ProviderKind::OpenaiCompatible, "not a url"), "your AI provider");
    }

    #[test]
    fn serializes_for_the_ui() {
        let v = serde_json::to_value(describe(KEY, ctx(ProviderKind::Anthropic, true))).unwrap();
        assert_eq!(v["kind"], "apiKey");
        assert_eq!(v["selfHealing"], false);
        assert_eq!(v["fixes"][0], serde_json::json!({"kind": "settings", "label": "Replace API key"}));
        assert_eq!(v["fixes"][1], serde_json::json!({"kind": "link", "label": "Open Claude Console", "target": "console_keys"}));
        let flaky = serde_json::to_value(describe("", ctx(ProviderKind::Anthropic, true))).unwrap();
        assert_eq!(flaky["kind"], "unreadable");
        assert_eq!(flaky["provider"], serde_json::Value::Null);
    }

    /// The browser preview's causes (src/dev/failure-causes.json) are this module's output for the
    /// preview's error texts, so the previews show the real wording. To rewrite the file after a
    /// wording change: `DAYTRACE_WRITE_PREVIEW_CAUSES=1 cargo test --lib preview_causes`.
    #[test]
    fn preview_causes_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/dev/failure-causes.json");
        let inputs: Vec<String> = {
            let text = std::fs::read_to_string(&path).expect("src/dev/failure-causes.json");
            let v: serde_json::Value = serde_json::from_str(&text).unwrap();
            v["errors"].as_array().unwrap().iter().map(|e| e.as_str().unwrap().to_string()).collect()
        };
        let causes = |provider: ProviderKind, recording: bool| -> serde_json::Map<String, serde_json::Value> {
            inputs.iter().map(|e| (e.clone(), serde_json::to_value(describe(e, ctx(provider, recording))).unwrap())).collect()
        };
        // The providers the previews use (?provider=claude_cli); the mock falls back to the first.
        let by_provider = |p: ProviderKind| serde_json::json!({"recording": causes(p, true), "paused": causes(p, false)});
        let want = serde_json::json!({
            "about": "Generated by llm/failure/cause.rs preview_causes_match. Do not edit by hand.",
            "errors": inputs,
            "anthropic": by_provider(ProviderKind::Anthropic),
            "claude_cli": by_provider(ProviderKind::ClaudeCli),
        });
        let text = serde_json::to_string_pretty(&want).unwrap() + "\n";
        if std::env::var_os("DAYTRACE_WRITE_PREVIEW_CAUSES").is_some() {
            std::fs::write(&path, &text).unwrap();
        }
        let have = std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n");
        assert!(have == text, "src/dev/failure-causes.json is out of date; run with DAYTRACE_WRITE_PREVIEW_CAUSES=1");
    }
}
