//! Update check: compares the running version with the latest GitHub release of
//! ArneVDA-AP/daytrace (`GET /repos/{repo}/releases/latest`, which skips drafts and pre-releases
//! and answers 404 while there is none). The release's notes are kept for "What's new". Dayflow
//! uses Sparkle to download and install updates; Daytrace does that with tauri-plugin-updater
//! (`autoupdate.rs`) when the release carries a signed latest.json, and otherwise tells the user
//! and opens the release page.

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::future::Future;

pub const REPO: &str = "ArneVDA-AP/daytrace";
/// At most one automatic check per day.
pub const CHECK_EVERY_SECS: i64 = 24 * 3600;
/// After a failed check (offline, rate limited) the next try comes this much sooner.
pub const RETRY_AFTER_ERROR_SECS: i64 = 3600;

pub fn api_url() -> String {
    format!("https://api.github.com/repos/{REPO}/releases/latest")
}

/// The release of one tag (for the notes of the running version).
pub fn tag_api_url(version: &str) -> String {
    format!("https://api.github.com/repos/{REPO}/releases/tags/v{}", version.trim_start_matches(['v', 'V']))
}

/// Release notes longer than this are cut (GitHub allows 125,000 characters).
pub const MAX_NOTES_CHARS: usize = 20_000;

fn releases_prefix() -> String {
    format!("https://github.com/{REPO}/releases/")
}

/// The answer of the releases endpoint.
#[derive(Debug, Clone, PartialEq)]
pub enum Fetched {
    /// Body of a 200 answer.
    Json(String),
    /// 404: no published release yet.
    NoReleases,
}

/// A semantic version: core numbers plus optional pre-release identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub core: [u64; 3],
    pub pre: Vec<String>,
}

impl Version {
    /// Parses `1.2.3`, `v1.2.3`, `1.2`, `1.2.3-beta.1` and `1.2.3+build` (build data ignored).
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix('v').or_else(|| s.strip_prefix('V')).unwrap_or(s);
        let s = s.split('+').next()?;
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, p.split('.').map(String::from).collect::<Vec<_>>()),
            None => (s, vec![]),
        };
        let parts: Vec<&str> = core.split('.').collect();
        if parts.is_empty() || parts.len() > 3 || pre.iter().any(|p| p.is_empty()) {
            return None;
        }
        let mut nums = [0u64; 3];
        for (i, p) in parts.iter().enumerate() {
            nums[i] = p.parse().ok()?;
        }
        Some(Version { core: nums, pre })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.core.cmp(&other.core).then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater, // 1.0.0 > 1.0.0-beta
            (false, true) => Ordering::Less,
            (false, false) => {
                for (a, b) in self.pre.iter().zip(&other.pre) {
                    let o = match (a.parse::<u64>(), b.parse::<u64>()) {
                        (Ok(x), Ok(y)) => x.cmp(&y),
                        (Ok(_), Err(_)) => Ordering::Less,
                        (Err(_), Ok(_)) => Ordering::Greater,
                        (Err(_), Err(_)) => a.cmp(b),
                    };
                    if o != Ordering::Equal {
                        return o;
                    }
                }
                self.pre.len().cmp(&other.pre.len())
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Release {
    pub tag: String,
    pub version: Version,
    pub url: String,
    /// The release notes (Markdown), if any.
    pub notes: Option<String>,
}

#[derive(Deserialize)]
struct ReleaseJson {
    tag_name: String,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    body: Option<String>,
}

/// Trimmed notes, cut at `MAX_NOTES_CHARS`; None when empty.
pub fn clean_notes(body: Option<&str>) -> Option<String> {
    let t = body?.replace("\r\n", "\n");
    let t = t.trim();
    if t.is_empty() {
        return None;
    }
    Some(if t.chars().count() > MAX_NOTES_CHARS { format!("{}…", t.chars().take(MAX_NOTES_CHARS).collect::<String>()) } else { t.to_string() })
}

/// Reads a release from the API's JSON. The page link is only kept when it points to this
/// repository's releases; otherwise it is built from the tag.
pub fn parse_release(json: &str) -> Result<Option<Release>> {
    let r: ReleaseJson = serde_json::from_str(json).map_err(|e| anyhow!("Unexpected answer from GitHub: {e}"))?;
    if r.draft || r.prerelease {
        return Ok(None);
    }
    let version = Version::parse(&r.tag_name).ok_or_else(|| anyhow!("The latest release has an unreadable version: {}", r.tag_name))?;
    let url = if r.html_url.starts_with(&releases_prefix()) && !r.html_url.contains(char::is_whitespace) {
        r.html_url
    } else {
        format!("{}tag/{}", releases_prefix(), r.tag_name)
    };
    let notes = clean_notes(r.body.as_deref());
    Ok(Some(Release { tag: r.tag_name, version, url, notes }))
}

/// Whether `url` may be opened as a release page.
pub fn is_release_url(url: &str) -> bool {
    url.starts_with(&releases_prefix()) && !url.contains(char::is_whitespace)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct UpdateStatus {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub release_url: Option<String>,
    pub available: bool,
    pub no_releases: bool,
    /// The last check that reached GitHub (the release fields are from then).
    pub checked_at: Option<i64>,
    pub error: Option<String>,
    /// The last check, also one that failed.
    pub attempted_at: Option<i64>,
    /// Notes of the latest release (Markdown), shown by "What's new" after the update.
    pub release_notes: Option<String>,
}

/// The status to keep after a check: a failed check only adds its error and time, so the
/// release found earlier (and the tray's "Version X available") stays.
pub fn merge(previous: &UpdateStatus, fresh: UpdateStatus) -> UpdateStatus {
    if fresh.error.is_none() {
        return UpdateStatus { attempted_at: fresh.checked_at, ..fresh };
    }
    UpdateStatus {
        current_version: fresh.current_version,
        error: fresh.error,
        attempted_at: fresh.checked_at,
        ..previous.clone()
    }
}

/// Runs one check with the given fetcher (injected so tests make no network calls).
pub async fn check<F, Fut>(current: &str, now: i64, fetch: F) -> UpdateStatus
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<Fetched>>,
{
    let mut st = UpdateStatus { current_version: current.to_string(), checked_at: Some(now), ..Default::default() };
    let fetched = match fetch().await {
        Ok(f) => f,
        Err(e) => {
            st.error = Some(format!("Couldn’t check for updates: {e:#}"));
            return st;
        }
    };
    match fetched {
        Fetched::NoReleases => st.no_releases = true,
        Fetched::Json(body) => match parse_release(&body) {
            Ok(Some(r)) => {
                st.available = Version::parse(current).is_some_and(|c| r.version > c);
                st.latest_version = Some(r.tag.trim_start_matches(['v', 'V']).to_string());
                st.release_url = Some(r.url);
                st.release_notes = r.notes;
            }
            Ok(None) => st.no_releases = true,
            Err(e) => st.error = Some(format!("{e:#}")),
        },
    }
    st
}

/// Whether an automatic check is due: a day after the last check, an hour after a failed one.
pub fn due(last: &UpdateStatus, now: i64) -> bool {
    let (at, every) = match (last.error.is_some(), last.attempted_at.or(last.checked_at)) {
        (true, Some(t)) => (t, RETRY_AFTER_ERROR_SECS),
        (_, _) => match last.checked_at {
            None => return true,
            Some(t) => (t, CHECK_EVERY_SECS),
        },
    };
    now - at >= every || now < at
}

/// The real fetcher: GitHub's REST API over HTTPS.
pub async fn fetch_github(current: &str) -> Result<Fetched> {
    fetch_url(&api_url(), current).await
}

/// The release of the running version's tag (`v<version>`), for its notes.
pub async fn fetch_github_tag(current: &str) -> Result<Fetched> {
    fetch_url(&tag_api_url(current), current).await
}

async fn fetch_url(url: &str, current: &str) -> Result<Fetched> {
    let http = reqwest::Client::builder().timeout(std::time::Duration::from_secs(20)).build()?;
    let r = http
        .get(url)
        .header("User-Agent", format!("Daytrace/{current}"))
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .send()
        .await
        .map_err(|e| anyhow!("could not reach GitHub ({e})"))?;
    match r.status().as_u16() {
        200 => Ok(Fetched::Json(r.text().await?)),
        404 => Ok(Fetched::NoReleases),
        403 | 429 => Err(anyhow!("GitHub is rate limiting requests; try again later")),
        s => Err(anyhow!("GitHub answered HTTP {s}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn compares_versions() {
        assert!(v("0.2.2") > v("0.2.1"));
        assert!(v("v0.10.0") > v("0.9.9"));
        assert!(v("1.0") == v("1.0.0"));
        assert!(v("1.0.0") > v("1.0.0-beta.2"));
        assert!(v("1.0.0-beta.10") > v("1.0.0-beta.2"));
        assert!(v("1.0.0-beta") > v("1.0.0-alpha.1"));
        assert!(v("1.0.0-alpha.1") > v("1.0.0-alpha"));
        assert!(v("1.0.0+build.5") == v("1.0.0"));
        assert_eq!(Version::parse("latest"), None);
        assert_eq!(Version::parse("1.2.3.4"), None);
        assert_eq!(Version::parse("1.2.3-"), None);
    }

    const JSON: &str = r#"{"url":"https://api.github.com/repos/ArneVDA-AP/daytrace/releases/1","html_url":"https://github.com/ArneVDA-AP/daytrace/releases/tag/v0.3.0","tag_name":"v0.3.0","name":"Daytrace 0.3.0","draft":false,"prerelease":false,"published_at":"2026-10-20T10:00:00Z","assets":[]}"#;

    #[test]
    fn parses_release_json() {
        let r = parse_release(JSON).unwrap().unwrap();
        assert_eq!(r.tag, "v0.3.0");
        assert_eq!(r.version, v("0.3.0"));
        assert_eq!(r.url, "https://github.com/ArneVDA-AP/daytrace/releases/tag/v0.3.0");
        assert_eq!(r.notes, None, "no body");
        let with_body = JSON.replace(r#""assets":[]"#, r#""assets":[],"body":"\r\n## Fixes\r\n- Pause from a script\r\n""#);
        assert_eq!(parse_release(&with_body).unwrap().unwrap().notes.as_deref(), Some("## Fixes\n- Pause from a script"));
        assert_eq!(clean_notes(Some("  ")), None);
        assert_eq!(clean_notes(Some(&"x".repeat(MAX_NOTES_CHARS + 5))).unwrap().chars().count(), MAX_NOTES_CHARS + 1);
        assert_eq!(tag_api_url("0.3.0"), "https://api.github.com/repos/ArneVDA-AP/daytrace/releases/tags/v0.3.0");
        // A link elsewhere is not trusted; it is rebuilt from the tag.
        let odd = JSON.replace("https://github.com/ArneVDA-AP/daytrace/releases/tag/v0.3.0", "https://example.com/x");
        assert_eq!(parse_release(&odd).unwrap().unwrap().url, "https://github.com/ArneVDA-AP/daytrace/releases/tag/v0.3.0");
        assert_eq!(parse_release(&JSON.replace(r#""prerelease":false"#, r#""prerelease":true"#)).unwrap(), None);
        assert!(parse_release("<html>").is_err());
        assert!(parse_release(r#"{"tag_name":"nightly"}"#).is_err());
        assert!(is_release_url("https://github.com/ArneVDA-AP/daytrace/releases/tag/v1"));
        assert!(!is_release_url("https://github.com/other/daytrace/releases/tag/v1"));
    }

    #[tokio::test]
    async fn checks_with_injected_fetcher() {
        let st = check("0.2.1", 100, || async { Ok(Fetched::Json(JSON.to_string())) }).await;
        assert!(st.available);
        assert_eq!(st.latest_version.as_deref(), Some("0.3.0"));
        assert_eq!(st.checked_at, Some(100));

        let st = check("0.3.0", 100, || async { Ok(Fetched::Json(JSON.to_string())) }).await;
        assert!(!st.available, "same version");
        let st = check("0.4.0", 100, || async { Ok(Fetched::Json(JSON.to_string())) }).await;
        assert!(!st.available, "running a newer build");

        let st = check("0.2.1", 100, || async { Ok(Fetched::NoReleases) }).await;
        assert!(st.no_releases && !st.available && st.error.is_none());

        let st = check("0.2.1", 100, || async { Err(anyhow!("could not reach GitHub")) }).await;
        assert!(st.error.as_deref().unwrap().contains("could not reach GitHub"));
        assert!(!st.available);
    }

    #[test]
    fn checks_once_a_day() {
        let mut st = UpdateStatus::default();
        assert!(due(&st, 1000));
        st.checked_at = Some(1000);
        assert!(!due(&st, 1000 + 3600));
        assert!(due(&st, 1000 + CHECK_EVERY_SECS));
        assert!(due(&st, 10), "clock went backwards");
    }

    #[tokio::test]
    async fn a_failed_check_keeps_the_release_and_retries_sooner() {
        let found = merge(&UpdateStatus::default(), check("0.2.1", 1000, || async { Ok(Fetched::Json(JSON.to_string())) }).await);
        assert!(found.available && found.attempted_at == Some(1000));
        // Offline a day later: the known release stays, only the error and the time are new.
        let later = 1000 + CHECK_EVERY_SECS;
        let failed = merge(&found, check("0.2.1", later, || async { Err(anyhow!("could not reach GitHub")) }).await);
        assert!(failed.available);
        assert_eq!(failed.latest_version.as_deref(), Some("0.3.0"));
        assert_eq!(failed.release_url, found.release_url);
        assert_eq!(failed.checked_at, Some(1000), "the last check that reached GitHub");
        assert_eq!(failed.attempted_at, Some(later));
        assert!(failed.error.is_some());
        assert!(!due(&failed, later + 600));
        assert!(due(&failed, later + RETRY_AFTER_ERROR_SECS), "retried after an hour, not a day");
        // The next good check clears the error.
        let ok = merge(&failed, check("0.2.1", later + 3600, || async { Ok(Fetched::NoReleases) }).await);
        assert!(ok.error.is_none() && ok.no_releases && !ok.available);
        assert!(!due(&ok, later + 7200));
    }
}
