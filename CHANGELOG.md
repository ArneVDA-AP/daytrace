# Changelog

Each release has a `## <version>` section. When a version is tagged, rename "Unreleased" to its
number: the release workflow uses that section as the GitHub release notes, and Daytrace shows it
in "What's new" after an update (from GitHub, or from this file when GitHub can't be reached).

## Unreleased

### Added
- Pause and resume from outside the window: `daytrace.exe --pause 30` (or `tomorrow`, `indefinite`), `daytrace.exe --resume`, `daytrace-mcp cli pause|resume`, and `daytrace://pause?minutes=30` links. Links can only pause.
- Updates install by themselves while the PC is idle or locked, and Daytrace starts again in the tray. Turn this off in Settings → Updates and about to get a notification instead.
- "What's new" after an update.
- "Copy debug info" in Settings → Updates and about: recent batches, model calls and the end of the log, with API keys, your user folder and window titles left out.
- Cards record which provider and model wrote them, and whether it was the fallback provider.

### Changed
- Reprocessing a day, a card or a group of failed cards sends its requests directly, also with the Batch API on, so it finishes in minutes instead of hours (at the regular price).
- The Claude Code and Codex one-step path keeps every request under the 90k input-token ceiling too, with fewer screenshots when needed.

## 0.2.1

- Low reasoning effort by default for Claude models (fewer billed output tokens).
- Capture follows the monitor of the active window.
- Each card's log is built from all observations in its own span; the model sees the full span of the cards it rewrites (up to 2 hours).
- A card in the Idle category is titled Idle.

## 0.2.0

- Idle guard: after 2 minutes without keyboard or mouse input, and with no video or call keeping the display on, no screenshots are stored.
- Optional Message Batches API for Claude (50% cheaper; cards arrive minutes later).
- Every request is checked against a ceiling of 90k input tokens.
- Cheaper card prompts, and batches wait for an API key instead of failing.
- Faster frame scaling and a lighter window that is closed to the tray.
