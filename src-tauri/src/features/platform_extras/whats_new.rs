//! "What's new" once after an update (Dayflow: Views/UI/WhatsNewView.swift, release highlights
//! shown once per version). The notes are the GitHub release body of the running version: kept
//! by the update check (`update.rs`) when it found this version, or fetched by tag. Offline, the
//! version's section of the CHANGELOG.md bundled with the app is shown instead. Nothing is shown
//! on a fresh install (the first-run setup comes first) or when no notes exist.

use super::update::{self, Fetched, UpdateStatus, Version};
use anyhow::Result;
use serde::Serialize;

/// CHANGELOG.md at the repository root, bundled with the app. The release workflow uses the
/// same section as the release body.
pub const CHANGELOG: &str = include_str!("../../../../CHANGELOG.md");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NotesSource {
    /// The GitHub release.
    Release,
    /// The bundled CHANGELOG.md.
    Changelog,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsNew {
    pub version: String,
    /// Markdown.
    pub notes: String,
    pub source: NotesSource,
    /// The release page, when the notes came from it.
    pub release_url: Option<String>,
}

/// Whether the notes of `current` are due: the first-run setup is done and this version is newer
/// than the last one whose notes were shown. No record means an update from a version without
/// this dialog (a fresh install records its version at startup).
pub fn due(seen: Option<&str>, current: &str, setup_done: bool) -> bool {
    if !setup_done {
        return false;
    }
    match seen {
        None => true,
        Some(s) => match (Version::parse(s), Version::parse(current)) {
            (Some(a), Some(b)) => b > a,
            _ => s.trim() != current,
        },
    }
}

/// The version a CHANGELOG heading names: `## 0.3.0`, `## [0.3.0] - 2026-10-20`, `## v0.3.0`.
fn heading_version(line: &str) -> Option<Version> {
    let rest = line.strip_prefix("## ")?.trim();
    let token = rest.split_whitespace().next()?.trim_matches(|c| c == '[' || c == ']');
    Version::parse(token)
}

/// The lines under the `## <version>` heading of `changelog`, up to the next `## ` heading.
pub fn changelog_section(changelog: &str, version: &str) -> Option<String> {
    let want = Version::parse(version)?;
    let mut out: Option<Vec<&str>> = None;
    for line in changelog.lines() {
        if line.starts_with("## ") {
            if out.is_some() {
                break;
            }
            if heading_version(line).as_ref() == Some(&want) {
                out = Some(Vec::new());
            }
            continue;
        }
        if let Some(lines) = out.as_mut() {
            lines.push(line);
        }
    }
    update::clean_notes(Some(&out?.join("\n")))
}

/// The notes for `current`: the stored check when it was about this version, else the release
/// fetched by tag (`fetched`, None when it was not needed or not tried), else the CHANGELOG.
pub fn resolve(current: &str, status: &UpdateStatus, fetched: Option<Result<Fetched>>, changelog: &str) -> Option<WhatsNew> {
    let this = |v: &str| Version::parse(v).zip(Version::parse(current)).is_some_and(|(a, b)| a == b);
    if status.latest_version.as_deref().is_some_and(this)
        && let Some(notes) = status.release_notes.clone()
    {
        return Some(WhatsNew { version: current.into(), notes, source: NotesSource::Release, release_url: status.release_url.clone() });
    }
    if let Some(Ok(Fetched::Json(body))) = fetched
        && let Ok(Some(r)) = update::parse_release(&body)
        && this(&r.tag)
        && let Some(notes) = r.notes
    {
        return Some(WhatsNew { version: current.into(), notes, source: NotesSource::Release, release_url: Some(r.url) });
    }
    changelog_section(changelog, current).map(|notes| WhatsNew { version: current.into(), notes, source: NotesSource::Changelog, release_url: None })
}

/// Whether the stored check already has this version's notes (no fetch needed).
pub fn stored_has_notes(status: &UpdateStatus, current: &str) -> bool {
    status.release_notes.is_some() && status.latest_version.as_deref().and_then(Version::parse).zip(Version::parse(current)).is_some_and(|(a, b)| a == b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::anyhow;

    const LOG: &str = "# Changelog\n\nIntro line.\n\n## Unreleased\n\n- Not yet\n\n## [0.3.0] - 2026-10-20\n\n### Added\n- Pause from a script\n\n## 0.2.1\n\n- Low reasoning\n";

    #[test]
    fn due_once_per_newer_version() {
        assert!(!due(None, "0.3.0", false), "the first-run setup comes first");
        assert!(due(None, "0.3.0", true), "updated from a version without the dialog");
        assert!(due(Some("0.2.1"), "0.3.0", true));
        assert!(!due(Some("0.3.0"), "0.3.0", true), "shown already");
        assert!(!due(Some("0.4.0"), "0.3.0", true), "went back to an older version");
    }

    #[test]
    fn reads_a_changelog_section() {
        assert_eq!(changelog_section(LOG, "0.3.0").as_deref(), Some("### Added\n- Pause from a script"));
        assert_eq!(changelog_section(LOG, "v0.2.1").as_deref(), Some("- Low reasoning"));
        assert_eq!(changelog_section(LOG, "0.1.0"), None);
        assert_eq!(changelog_section("## 0.3.0\n\n", "0.3.0"), None, "an empty section");
        // The bundled file parses (and has no section for a version that does not exist).
        assert_eq!(changelog_section(CHANGELOG, "999.0.0"), None);
    }

    fn release_json(tag: &str, body: &str) -> String {
        serde_json::json!({"tag_name": tag, "html_url": format!("https://github.com/{}/releases/tag/{tag}", update::REPO), "draft": false, "prerelease": false, "body": body}).to_string()
    }

    #[test]
    fn notes_from_the_check_then_the_tag_then_the_changelog() {
        let stored = UpdateStatus {
            latest_version: Some("0.3.0".into()),
            release_notes: Some("From the check".into()),
            release_url: Some("https://github.com/ArneVDA-AP/daytrace/releases/tag/v0.3.0".into()),
            ..Default::default()
        };
        assert!(stored_has_notes(&stored, "0.3.0"));
        let w = resolve("0.3.0", &stored, None, LOG).unwrap();
        assert_eq!((w.notes.as_str(), w.source), ("From the check", NotesSource::Release));
        assert!(w.release_url.is_some());

        // The check was about another version: the tag's release.
        let other = UpdateStatus { latest_version: Some("0.3.1".into()), ..stored.clone() };
        assert!(!stored_has_notes(&other, "0.3.0"));
        let w = resolve("0.3.0", &other, Some(Ok(Fetched::Json(release_json("v0.3.0", "From the tag")))), LOG).unwrap();
        assert_eq!((w.notes.as_str(), w.source), ("From the tag", NotesSource::Release));

        // Offline, or a release without notes: the bundled section.
        for fetched in [Some(Err(anyhow!("offline"))), Some(Ok(Fetched::NoReleases)), Some(Ok(Fetched::Json(release_json("v0.3.0", "  ")))), None] {
            let w = resolve("0.3.0", &UpdateStatus::default(), fetched, LOG).unwrap();
            assert_eq!((w.notes.as_str(), w.source, w.release_url), ("### Added\n- Pause from a script", NotesSource::Changelog, None));
        }
        // A release of another tag does not count.
        let w = resolve("0.3.0", &UpdateStatus::default(), Some(Ok(Fetched::Json(release_json("v0.2.1", "Old")))), LOG).unwrap();
        assert_eq!(w.source, NotesSource::Changelog);
        // Nothing anywhere: no dialog.
        assert_eq!(resolve("0.5.0", &UpdateStatus::default(), Some(Err(anyhow!("offline"))), LOG), None);
    }
}
