# Changelog

Each release has a `## <version>` section. When a version is tagged, rename "Unreleased" to its
number: the release workflow uses that section as the GitHub release notes, and Daytrace shows it
in "What's new" after an update (from GitHub, or from this file when GitHub can't be reached).

## 0.3.0

### New in the window
- A new design: a rail with the sections and the recorder status, Pause and Resume in the rail, light, dark and Windows Contrast themes, and Ctrl+K to jump to a section, a setting or a day ("monday", "12 oct").
- Timeline: a strip of the whole day with the time you were idle or not recording, a Week view with the seven days side by side, and a review mode that plays each unrated card so you can rate it with 1, 2 or 3.
- Edit a card's title and category or delete it, rate it as focused, neutral or distracted, and leave feedback on how it was written. Your edits stay when Daytrace rewrites the cards around them.
- Reprocess a card or a whole day. These requests go out directly, also with the Batch API on, so they finish in minutes (at the regular price).
- A timelapse of each card's screenshots, and a month calendar to pick a day.
- App and site icons next to the names on cards, in Daily and in Weekly. Site icons are only looked up when you turn that on.
- Daily: a standup recap of the day (done, next, blockers), with a model of its own if you want, time per category, an activity grid and distractions. The recap can be written by itself each morning and saved as Markdown to a folder you pick, for example to read it on your phone.
- Weekly: charts of the week, a focus heat map by hour, and "Save as PNG…" on every chart.
- Goals: a focus target and a distraction limit per day, a checklist that Daytrace checks against what you did, and an optional prompt for the day's goal in the morning.
- Each category can count as focus, as distraction or as neither; Daily, Weekly and Goals follow that choice. Every percentage is a share of active time (recorded time without idle time).
- Chat: ask about your days. Answers cite the cards they come from and can show a chart; a memory of your preferences, which you can edit, carries over between conversations. Chat also runs through the Claude Code CLI.
- Agents: start AgentPlayback to see your Claude Code and Codex sessions of the day, with their cost at API prices. It runs on this PC and stays loaded while you switch sections; its links open in your browser.
- First-run setup, "What's new" after an update, a dot on Daily (and on the taskbar button) when a new recap is ready.

### Models
- Claude Code CLI and Codex CLI as providers: they use your Claude or ChatGPT subscription.
- A fallback provider takes over when the main one fails, and cards say which model wrote them.
- The title and summary instructions of the prompts can be edited in Settings.
- When analysis fails, the rail and a banner say why in plain words (a refused key, no credits, a usage limit, a signed-out CLI, a busy server) with the fix as a button.

### AI tools
- `daytrace-mcp` gives Claude Code, Claude Desktop and other MCP clients read access to your timeline, and works as a command line (`daytrace-mcp cli today`). The installer puts it on your PATH.
- One-click setup for Claude Code and Claude Desktop in Settings → AI tools, and a Claude Code plugin with skills for a standup, a weekly review and questions about your day.
- "Allow edits" (off by default) lets these tools rename, recategorize and delete activities and set day goals, through the running app; every change is logged.

### Recording and the PC
- Pause for 15, 30 or 60 minutes, until tomorrow or until you resume, also from the tray, from a script (`daytrace.exe --pause 30`, `--resume`) or with `daytrace://pause?minutes=30` links. Links can only pause.
- Pick private apps from the ones you used. The tray icon shows whether Daytrace is recording, paused or failing.
- After 15 idle minutes the window goes back to today (days start at 4 AM, as before).
- Notifications when the recap or the first cards of the day are ready, and for problems that need you; each kind can be turned off.
- Updates install by themselves while the PC is idle or locked, and Daytrace starts again in the tray. Turn this off in Settings → Updates and about to get a notification instead.
- Daily database backups with recovery, a storage panel with a size limit, and "Copy debug info" (API keys, your user folder and window titles left out).
- Screenshots are still deleted after the retention period, but the text descriptions of the screen and the token counts are kept, so chat and recaps can use older days.

### Fixed
- Keyboard shortcuts work on every keyboard layout (Ctrl+1 on Belgian AZERTY).
- Menus no longer hide the rest of the window from screen readers while they are open.
- The Claude Code and Codex path keeps every request under the 90k input-token ceiling too, with fewer screenshots when needed.

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
