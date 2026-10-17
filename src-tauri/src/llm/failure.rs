//! Sorts analysis failures into plain-language causes (Dayflow TimelineFailureClassifier.swift).
//! The rules and their order are Dayflow's; the copy is adapted to Windows and Daytrace. The
//! card message goes on the failed timeline card; `is_actionable` tells whether the user has to
//! do something (sign in, install, top up) before analysis can work again.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
    // The user has to act.
    ProviderLoginExpired,
    CliNotInstalled,
    CliOutdated,
    LocalEngineOffline,
    LocalModelMissing,
    ApiKeyProblem,
    UsageLimitHit,
    OutOfCredits,
    GeoBlocked,
    AccountBlocked,
    ModelMisconfigured,
    NotConfigured,
    // Daytrace retries these on its own.
    Transient,
    ModelFlaky,
    Unknown,
}

impl FailureKind {
    pub fn is_actionable(self) -> bool {
        !matches!(self, FailureKind::Transient | FailureKind::ModelFlaky | FailureKind::Unknown)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub kind: FailureKind,
    /// Provider named by the error text itself ("Claude", "ChatGPT", ...), when it names one.
    pub provider: Option<&'static str>,
}

/// Ordered: the first matching rule wins (Dayflow's table, plus Windows-specific patterns).
const RULES: &[(FailureKind, &[&str])] = &[
    // Daytrace's own checks on the model's reply; they can quote a card title, so they come first.
    (
        FailureKind::ModelFlaky,
        &[
            "minutes long", "empty title", "last segment", "segment end time", "segment out of bounds", "missing coverage",
            "gap detected", "do not cover all", "invalid duration", "first segment must start", "overlap detected",
            "no cards returned", "covers a genuine gap", "outside the supplied source", "activity cards overlap",
            "could not parse json", "no valid segments",
        ],
    ),
    (
        FailureKind::ProviderLoginExpired,
        &[
            "could not be refreshed", "log out and sign in", "please run /login", "run /login", "not logged in", "oauth",
            "token has been revoked", "authentication token", "sso session", "aws sso login",
        ],
    ),
    (FailureKind::CliNotInstalled, &["cli not found", "enoent", "requires git-bash", "git-bash"]),
    (FailureKind::CliOutdated, &["requires a newer version of codex", "newer version of codex"]),
    (
        FailureKind::ModelMisconfigured,
        &["is not supported when using codex with a chatgpt account", "is not found for api version", "not_found_error", "model_not_found", "unknown model"],
    ),
    (FailureKind::LocalEngineOffline, &["could not reach the model server"]),
    (FailureKind::LocalModelMissing, &["ollama api request failed with status 404", "no models loaded", "context size has been exceeded"]),
    (
        FailureKind::UsageLimitHit,
        &["hit your usage limit", "hit your limit", "usage limit", "session limit", "weekly limit", "spend limit", "accountquotaexceeded", "usage quota"],
    ),
    (
        FailureKind::OutOfCredits,
        &["prepayment credits", "insufficient balance", "spending cap", "dunning", "payment required", "out of credits", "credit balance"],
    ),
    (
        FailureKind::ApiKeyProblem,
        &["api key not found", "api key expired", "has been suspended", "api key not valid", "invalid api key", "service account", "no anthropic api key", "authentication_error", "invalid x-api-key"],
    ),
    (FailureKind::GeoBlocked, &["location is not supported"]),
    (FailureKind::AccountBlocked, &["denied access", "organization has disabled", "account is on hold"]),
    (FailureKind::NotConfigured, &["no llm provider configured"]),
    (
        FailureKind::Transient,
        &[
            "timed out", "reconnecting", "network connection", "internet connection", "tls error", "ssl error", "could not connect",
            "unable to connect", "connection refused", "econnreset", "enotfound", "socket", "hostname", "502", "503", "529",
            "upstream", "service unavailable", "internal error", "high demand", "at capacity", "quota exceeded",
            "too many requests", "overloaded", "stream disconnected", "message too long", "cancelled", "error sending request",
            "http 429", "rate limit",
        ],
    ),
    (
        FailureKind::ModelFlaky,
        &["failed to decode", "be read because", "failed to parse", "reading additional input", "failed to load", "no stdout to parse", "returned no answer", "unusable"],
    ),
];

/// The provider name the card copy uses for a configured provider.
pub fn provider_name(kind: crate::config::ProviderKind) -> Option<&'static str> {
    use crate::config::ProviderKind as K;
    match kind {
        K::Anthropic | K::ClaudeCli => Some("Claude"),
        K::CodexCli => Some("ChatGPT"),
        K::Ollama | K::LmStudio => Some("Ollama/LM Studio"),
        K::OpenaiCompatible => None,
    }
}

pub fn classify(message: &str) -> Classification {
    let lower = message.to_lowercase();
    Classification { kind: kind_for(&lower), provider: provider_in(&lower) }
}

fn kind_for(lower: &str) -> FailureKind {
    let t = lower.trim();
    // Raw model output as the error text: a flaky reply.
    if t.is_empty() || t.starts_with('[') || t.starts_with('{') || t.starts_with(']') || t.starts_with('`') {
        return FailureKind::ModelFlaky;
    }
    // A rejected key over HTTP (the API providers) before the broad patterns below.
    if t.contains("http 401") || (t.contains("http 403") && t.contains("key")) {
        return FailureKind::ApiKeyProblem;
    }
    RULES.iter().find(|(_, pats)| pats.iter().any(|p| lower.contains(p))).map(|(k, _)| *k).unwrap_or(FailureKind::Unknown)
}

fn provider_in(lower: &str) -> Option<&'static str> {
    if lower.contains("codex") || lower.contains("chatgpt") || lower.contains("could not be refreshed") {
        return Some("ChatGPT");
    }
    if lower.contains("aws sso") || lower.contains("sso session") {
        return Some("AWS");
    }
    if lower.contains("claude") || lower.contains("/login") || lower.contains("anthropic") {
        return Some("Claude");
    }
    if lower.contains("ollama") || lower.contains("lmstudio") || lower.contains("lm studio") {
        return Some("Ollama/LM Studio");
    }
    if lower.contains("gemini") || lower.contains("generativelanguage") {
        return Some("Gemini");
    }
    None
}

impl Classification {
    /// One or two plain sentences for the failed card (Dayflow cardMessage, for Windows).
    pub fn card_message(&self, fallback_provider: Option<&str>) -> String {
        let provider = self.provider.or(fallback_provider).unwrap_or("your AI provider");
        let catch_up = "Daytrace will catch up automatically once that's done.";
        match self.kind {
            FailureKind::ProviderLoginExpired => match provider {
                "Claude" => format!("Claude is signed out. Open a terminal (PowerShell), run `claude`, then type `/login` to sign back in. {catch_up}"),
                "ChatGPT" => format!("ChatGPT is signed out. Run `codex login` in a terminal to sign back in. {catch_up}"),
                "AWS" => format!("AWS is signed out. Run `aws sso login` in a terminal to sign back in. {catch_up}"),
                p => format!("{p} is signed out. Sign in again, or check the API key in Settings, under Model. {catch_up}"),
            },
            FailureKind::CliNotInstalled => {
                let install = match provider {
                    "Claude" => " Claude Code on Windows also needs Git for Windows.",
                    _ => "",
                };
                format!("Daytrace couldn't find {provider}'s command-line tool on this PC. Reinstall it, set its path in Settings, or switch providers.{install} {catch_up}")
            }
            FailureKind::CliOutdated => {
                format!("Your Codex command-line tool is too old for the model Daytrace uses. Run `npm install -g @openai/codex@latest` in a terminal to update it. {catch_up}")
            }
            FailureKind::LocalEngineOffline => format!("Daytrace couldn't reach Ollama or LM Studio on this PC. Open it. {catch_up}"),
            FailureKind::LocalModelMissing => {
                format!("Your local model isn't loaded. Open Ollama or LM Studio and load the model you picked in Daytrace. {catch_up}")
            }
            FailureKind::ApiKeyProblem => format!("Your {provider} API key isn't working. Add a new key in Settings, under Model. {catch_up}"),
            FailureKind::UsageLimitHit => {
                format!("You've used up your {provider} plan's usage for now. Daytrace will catch up automatically once your limit resets.")
            }
            FailureKind::OutOfCredits => format!("Your {provider} account is out of credits. Top it up, or switch providers in Settings. {catch_up}"),
            FailureKind::GeoBlocked => format!("{provider} isn't available in your region. Switch to another provider in Settings. {catch_up}"),
            FailureKind::AccountBlocked => {
                format!("{provider} blocked access for this account. A new API key or a different provider in Settings usually fixes it. {catch_up}")
            }
            FailureKind::ModelMisconfigured => {
                format!("The AI model Daytrace tried to use isn't available on your {provider} account. Pick a different one in Settings, under Model. {catch_up}")
            }
            FailureKind::NotConfigured => format!("No AI provider is connected yet. Connect one in Settings, under Model. {catch_up}"),
            FailureKind::Transient => {
                format!("Daytrace couldn't reach {provider} because the connection dropped or its servers were busy. Daytrace will try again automatically.")
            }
            FailureKind::ModelFlaky => {
                "The AI's reply came back jumbled, so Daytrace couldn't turn it into cards. This usually sorts itself out, and Daytrace will try again automatically.".into()
            }
            FailureKind::Unknown => "Something went wrong while turning this time into cards. Daytrace will try again automatically.".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_cli_failures() {
        let c = classify("Claude Code CLI: Invalid API key · Please run /login");
        assert_eq!(c.kind, FailureKind::ProviderLoginExpired);
        assert_eq!(c.provider, Some("Claude"));
        assert!(c.card_message(None).contains("run `claude`, then type `/login`"));

        let c = classify("Claude Code CLI: You've hit your limit · resets 3pm (Europe/Brussels)");
        assert_eq!(c.kind, FailureKind::UsageLimitHit);
        assert!(c.kind.is_actionable());

        let c = classify("Claude Code CLI not found. Install it (in PowerShell: ...), sign in once in a terminal, or set its path in Settings.");
        assert_eq!(c.kind, FailureKind::CliNotInstalled);
        assert!(c.card_message(None).contains("Git for Windows"));

        assert_eq!(classify("Codex CLI: ERROR: You've hit your usage limit.").provider, Some("ChatGPT"));
        assert_eq!(classify("Claude Code CLI: Not logged in · Please run /login").kind, FailureKind::ProviderLoginExpired);
        assert_eq!(classify("CLI process timed out after 300 seconds").kind, FailureKind::Transient);
        assert!(!classify("CLI process timed out after 300 seconds").kind.is_actionable());
    }

    #[test]
    fn validation_messages_win_over_words_in_titles() {
        // The quoted title mentions a "usage limit"; it is still a model reply problem.
        let c = classify("Card 1 'Reading about usage limit docs' is only 8.0 minutes long");
        assert_eq!(c.kind, FailureKind::ModelFlaky);
        assert_eq!(classify("{\"cards\": [").kind, FailureKind::ModelFlaky);
        assert_eq!(classify("").kind, FailureKind::ModelFlaky);
    }

    #[test]
    fn api_errors() {
        assert_eq!(classify(r#"HTTP 401: {"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#).kind, FailureKind::ApiKeyProblem);
        assert_eq!(classify("HTTP 529: overloaded").kind, FailureKind::Transient);
        assert_eq!(classify("Could not reach the model server: error sending request").kind, FailureKind::LocalEngineOffline);
        assert_eq!(classify("something odd").kind, FailureKind::Unknown);
    }
}
