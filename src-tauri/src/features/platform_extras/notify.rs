//! Windows notifications (toasts). Ports the Dayflow notifications that apply on Windows:
//! "first cards ready" (NotificationService.notifyFirstCardsReady), "daily recap ready"
//! (scheduleDailyRecapReadyNotification), and the failure card for problems the user has to fix
//! (TimelineFailureClassifier + LLMService.emitTimelineFailureToast), plus recording problems, a
//! missing API key and new releases. Like Dayflow's shouldEmitTimelineFailureToast, each kind is
//! shown at most once per logical day; on top of that no two notifications come within
//! `MIN_GAP_SECS` of each other.

use crate::config::ProviderKind;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct NotificationPrefs {
    /// The automatic daily recap was written.
    pub recap_ready: bool,
    /// The first timeline cards after setup are ready (shown once).
    pub first_cards: bool,
    /// Screenshots keep failing.
    pub recording_problems: bool,
    /// Analysis fails in a way that needs the user (bad key, no credits, local model offline) or
    /// keeps failing.
    pub analysis_problems: bool,
    /// Screenshots wait for an API key.
    pub missing_api_key: bool,
    /// A new Daytrace version was released.
    pub updates: bool,
}

impl Default for NotificationPrefs {
    fn default() -> Self {
        NotificationPrefs { recap_ready: true, first_cards: true, recording_problems: true, analysis_problems: true, missing_api_key: true, updates: true }
    }
}

/// Where a click on the notification takes the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Timeline,
    Daily,
    Settings,
    Release,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    RecapReady,
    FirstCards,
    RecordingProblem,
    Analysis(Failure),
    AnalysisRepeated,
    MissingKey,
    Update,
    Test,
}

impl Kind {
    /// Key for the once-a-day limit.
    pub fn key(&self) -> String {
        match self {
            Kind::RecapReady => "recap_ready".into(),
            Kind::FirstCards => "first_cards".into(),
            Kind::RecordingProblem => "recording_problem".into(),
            Kind::Analysis(f) => format!("analysis:{}", f.key()),
            Kind::AnalysisRepeated => "analysis_repeated".into(),
            Kind::MissingKey => "missing_key".into(),
            Kind::Update => "update".into(),
            Kind::Test => "test".into(),
        }
    }

    pub fn enabled(&self, p: &NotificationPrefs) -> bool {
        match self {
            Kind::RecapReady => p.recap_ready,
            Kind::FirstCards => p.first_cards,
            Kind::RecordingProblem => p.recording_problems,
            Kind::Analysis(_) | Kind::AnalysisRepeated => p.analysis_problems,
            Kind::MissingKey => p.missing_api_key,
            Kind::Update => p.updates,
            Kind::Test => true,
        }
    }

    /// Stable id per kind, used as the toast id so a click can be routed.
    pub fn toast_id(&self) -> i32 {
        match self {
            Kind::RecapReady => 1,
            Kind::FirstCards => 2,
            Kind::RecordingProblem => 3,
            Kind::Analysis(_) => 4,
            Kind::AnalysisRepeated => 5,
            Kind::MissingKey => 6,
            Kind::Update => 7,
            Kind::Test => 8,
        }
    }

    pub fn target(&self) -> Target {
        match self {
            Kind::RecapReady => Target::Daily,
            Kind::FirstCards | Kind::RecordingProblem | Kind::AnalysisRepeated | Kind::Test => Target::Timeline,
            Kind::Analysis(_) | Kind::MissingKey => Target::Settings,
            Kind::Update => Target::Release,
        }
    }

    pub fn target_for_toast_id(id: i32) -> Target {
        match id {
            1 => Target::Daily,
            4 | 6 => Target::Settings,
            7 => Target::Release,
            _ => Target::Timeline,
        }
    }
}

/// Failures the user has to fix (Dayflow's actionable TimelineFailureKind values that exist
/// with Daytrace's providers). Everything else is retried automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Failure {
    ApiKey,
    OutOfCredits,
    UsageLimit,
    ModelMissing,
    LocalOffline,
}

impl Failure {
    fn key(&self) -> &'static str {
        match self {
            Failure::ApiKey => "api_key",
            Failure::OutOfCredits => "out_of_credits",
            Failure::UsageLimit => "usage_limit",
            Failure::ModelMissing => "model_missing",
            Failure::LocalOffline => "local_offline",
        }
    }
}

/// Classifies a failed batch's error text. Pattern lists follow Dayflow's TimelineFailureClassifier
/// rules (first match wins), extended with the messages Daytrace's own client produces.
pub fn classify(message: &str, provider: ProviderKind) -> Option<Failure> {
    let lower = message.to_lowercase();
    let has = |pats: &[&str]| pats.iter().any(|p| lower.contains(p));
    // Daytrace's own checks on the model's reply can quote card text; they are never actionable.
    if has(&["could not parse json", "invalid json from provider", "cut off at max_tokens", "no cards with valid times"]) {
        return None;
    }
    if has(&["hit your usage limit", "hit your limit", "usage limit", "spend limit", "usage quota", "accountquotaexceeded"]) {
        return Some(Failure::UsageLimit);
    }
    if has(&["prepayment credits", "insufficient balance", "spending cap", "payment required", "out of credits", "credit balance", "insufficient_quota", "billing"]) {
        return Some(Failure::OutOfCredits);
    }
    if has(&[
        "http 401",
        "invalid x-api-key",
        "authentication_error",
        "api key not found",
        "api key expired",
        "has been suspended",
        "api key not valid",
        "invalid api key",
        "incorrect api key",
        "no anthropic api key",
        "no api key",
    ]) {
        return Some(Failure::ApiKey);
    }
    if has(&["not_found_error", "model_not_found", "model not found", "is not found for api version", "no models loaded", "ollama api request failed with status 404"])
        || (lower.contains("http 404") && lower.contains("model"))
    {
        return Some(Failure::ModelMissing);
    }
    let local = matches!(provider, ProviderKind::Ollama | ProviderKind::LmStudio);
    if local && has(&["could not reach", "connection refused", "error sending request", "could not connect", "unable to connect"]) {
        return Some(Failure::LocalOffline);
    }
    None
}

pub fn provider_name(p: ProviderKind) -> &'static str {
    match p {
        ProviderKind::Anthropic => "Claude",
        ProviderKind::Ollama => "Ollama",
        ProviderKind::LmStudio => "LM Studio",
        ProviderKind::OpenaiCompatible => "your AI provider",
    }
}

/// Title and body of a notification.
pub fn text(kind: Kind, provider: ProviderKind, detail: &str) -> (String, String) {
    let p = provider_name(provider);
    let (t, b): (String, String) = match kind {
        Kind::RecapReady => ("Your daily recap is ready".into(), "Click to open it in the Daily view.".into()),
        Kind::FirstCards => (
            "Your first Daytrace cards are ready".into(),
            "Take a look at what Daytrace picked up from your last few minutes of work.".into(),
        ),
        Kind::RecordingProblem => (
            "Daytrace can’t capture the screen".into(),
            format!("No screenshots for the last few minutes ({detail}). Daytrace keeps trying."),
        ),
        Kind::Analysis(Failure::ApiKey) => (
            format!("Your {p} API key isn’t working"),
            "It may be missing, expired, or suspended. Add a new key in Settings.".into(),
        ),
        Kind::Analysis(Failure::OutOfCredits) => (format!("{p} is out of credits"), "Top up your account, or switch providers in Settings.".into()),
        Kind::Analysis(Failure::UsageLimit) => (
            format!("You’ve hit {p}’s usage limit"),
            "Daytrace will catch up on its own when your limit resets.".into(),
        ),
        Kind::Analysis(Failure::ModelMissing) => (
            "Your selected model isn’t available".into(),
            format!("The model Daytrace tried to use isn’t available on {}. Pick a different one in Settings.", if p == "your AI provider" { "your provider".to_string() } else { p.to_string() }),
        ),
        Kind::Analysis(Failure::LocalOffline) => (
            "Ollama or LM Studio isn’t running".into(),
            "Daytrace can’t reach your local model. Open Ollama or LM Studio.".into(),
        ),
        Kind::AnalysisRepeated => (
            "Daytrace couldn’t turn your activity into cards".into(),
            format!("The last {detail} attempts failed. Your recording is saved, and Daytrace will try again automatically."),
        ),
        Kind::MissingKey => (
            "No API key set".into(),
            "Your recordings are saved, but nothing gets turned into cards until you add an API key in Settings.".into(),
        ),
        Kind::Update => (format!("Daytrace {detail} is available"), "Click to open the release page.".into()),
        Kind::Test => ("Notifications are on".into(), "This is how Daytrace notifications look.".into()),
    };
    (t, b)
}

/// Minimum time between any two notifications.
pub const MIN_GAP_SECS: i64 = 5 * 60;

/// Once per kind per logical day, and never two within `MIN_GAP_SECS`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct RateLimiter {
    pub last_day_by_kind: HashMap<String, String>,
    pub last_shown_at: Option<i64>,
}

/// What the limit says about a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    /// Already shown today: skip it.
    SameDay,
    /// Another notification was shown moments ago: try again later.
    TooSoon,
}

impl RateLimiter {
    pub fn check(&self, key: &str, day: &str, now: i64) -> Decision {
        if self.last_day_by_kind.get(key).map(String::as_str) == Some(day) {
            return Decision::SameDay;
        }
        if matches!(self.last_shown_at, Some(t) if now - t < MIN_GAP_SECS && now >= t) {
            return Decision::TooSoon;
        }
        Decision::Allow
    }

    pub fn would_allow(&self, key: &str, day: &str, now: i64) -> bool {
        self.check(key, day, now) == Decision::Allow
    }

    /// Records the notification when it may be shown; returns the decision.
    pub fn record(&mut self, key: &str, day: &str, now: i64) -> Decision {
        let d = self.check(key, day, now);
        if d == Decision::Allow {
            self.last_day_by_kind.insert(key.to_string(), day.to_string());
            self.last_shown_at = Some(now);
        }
        d
    }

    pub fn allow(&mut self, key: &str, day: &str, now: i64) -> bool {
        self.record(key, day, now) == Decision::Allow
    }
}

/// Recent analysis outcomes, newest first: `(failed, reason)`. Returns the failure to report:
/// an actionable kind from the newest failure, or `AnalysisRepeated` after `REPEAT` failures in a
/// row.
pub const REPEAT: usize = 3;

pub fn analysis_kind(outcomes: &[(bool, Option<String>)], provider: ProviderKind) -> Option<(Kind, usize)> {
    let streak = outcomes.iter().take_while(|(failed, _)| *failed).count();
    if streak == 0 {
        return None;
    }
    let reason = outcomes[0].1.as_deref().unwrap_or("");
    if let Some(f) = classify(reason, provider) {
        return Some((Kind::Analysis(f), streak));
    }
    (streak >= REPEAT).then_some((Kind::AnalysisRepeated, streak))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_failures() {
        let a = ProviderKind::Anthropic;
        assert_eq!(classify(r#"HTTP 401: {"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#, a), Some(Failure::ApiKey));
        assert_eq!(classify("HTTP 400: Your credit balance is too low to access the Anthropic API.", a), Some(Failure::OutOfCredits));
        assert_eq!(classify(r#"HTTP 404: {"error":{"type":"not_found_error","message":"model: claude-x"}}"#, a), Some(Failure::ModelMissing));
        assert_eq!(classify("Could not reach the model server: error sending request", ProviderKind::Ollama), Some(Failure::LocalOffline));
        // Network trouble with a cloud provider is transient: retried, no notification.
        assert_eq!(classify("Could not reach the model server: error sending request", a), None);
        assert_eq!(classify("HTTP 429: rate_limit_error", a), None);
        assert_eq!(classify("HTTP 529: overloaded", a), None);
        // A model reply quoting a card title must not be read as a key problem.
        assert_eq!(classify("Could not parse JSON from model output: 'invalid api key handling'", a), None);
    }

    #[test]
    fn picks_analysis_notification() {
        let a = ProviderKind::Anthropic;
        let f = |r: &str| (true, Some(r.to_string()));
        let ok = (false, None);
        assert_eq!(analysis_kind(&[ok.clone(), f("HTTP 401")], a), None, "a later success clears it");
        assert_eq!(analysis_kind(&[f("HTTP 401 invalid x-api-key")], a), Some((Kind::Analysis(Failure::ApiKey), 1)));
        assert_eq!(analysis_kind(&[f("HTTP 529"), f("HTTP 529")], a), None);
        assert_eq!(analysis_kind(&[f("HTTP 529"), f("HTTP 529"), f("timeout"), ok], a), Some((Kind::AnalysisRepeated, 3)));
        assert_eq!(analysis_kind(&[], a), None);
    }

    #[test]
    fn rate_limits_per_kind_and_day() {
        let mut r = RateLimiter::default();
        assert!(r.allow("a", "2026-10-17", 1000));
        assert!(!r.allow("a", "2026-10-17", 1000 + 3600), "same kind, same day");
        assert!(!r.allow("b", "2026-10-17", 1000 + 60), "too soon after another notification");
        assert!(r.allow("b", "2026-10-17", 1000 + MIN_GAP_SECS));
        assert!(r.allow("a", "2026-10-18", 1000 + 86400), "next day");
        // Survives a restart (persisted as JSON).
        let back: RateLimiter = serde_json::from_str(&serde_json::to_string(&r).unwrap()).unwrap();
        assert!(!back.would_allow("a", "2026-10-18", 1000 + 90000));
        // A clock that went backwards does not block notifications forever.
        assert!(back.would_allow("c", "2026-10-18", 10));
        assert_eq!(back.check("a", "2026-10-18", 1000 + 90000), Decision::SameDay);
        assert_eq!(back.check("c", "2026-10-18", 1000 + 86400 + 1), Decision::TooSoon);
    }

    #[test]
    fn prefs_and_routing() {
        let mut p = NotificationPrefs::default();
        assert!(Kind::RecapReady.enabled(&p));
        p.recap_ready = false;
        assert!(!Kind::RecapReady.enabled(&p));
        assert!(Kind::Test.enabled(&p));
        for k in [Kind::RecapReady, Kind::MissingKey, Kind::Update, Kind::Analysis(Failure::ApiKey), Kind::FirstCards] {
            assert_eq!(Kind::target_for_toast_id(k.toast_id()), k.target());
        }
        let p: NotificationPrefs = serde_json::from_str(r#"{"updates":false}"#).unwrap();
        assert!(!p.updates && p.recap_ready, "missing fields default to on");
    }

    #[test]
    fn texts() {
        let (t, b) = text(Kind::Analysis(Failure::ApiKey), ProviderKind::Anthropic, "");
        assert_eq!(t, "Your Claude API key isn’t working");
        assert!(b.contains("Settings"));
        let (_, b) = text(Kind::AnalysisRepeated, ProviderKind::Ollama, "4");
        assert!(b.starts_with("The last 4 attempts failed."));
        let (t, _) = text(Kind::Update, ProviderKind::Anthropic, "0.3.0");
        assert_eq!(t, "Daytrace 0.3.0 is available");
    }
}
