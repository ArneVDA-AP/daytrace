# Changelog

Each release has a `## <version>` section. When a version is tagged, rename "Unreleased" to its
number: the release workflow uses that section as the GitHub release notes, and Daytrace shows it
in "What's new" after an update (from GitHub, or from this file when GitHub can't be reached).

## 0.3.0

### New in the window
- A Daytrace app icon (the trace mark from the rail) instead of the default Tauri icon, on the window, taskbar, tray and installer.
- A new design: a rail with the sections and the recorder status, Pause and Resume in the rail, light, dark and Windows Contrast themes, and Ctrl+K to jump to a section, a setting or a day ("monday", "12 oct").
- Timeline: a strip of the whole day with the time you were idle or not recording, a Week view with the seven days side by side, and a review mode that plays each unrated card so you can rate it with 1, 2 or 3.
- Edit a card's title and category or delete it, rate it as focused, neutral or distracted, and leave feedback on how it was written. Your edits stay when Daytrace rewrites the cards around them.
- Reprocess a card or a whole day. These requests go out directly, also with the Batch API on, so they finish in minutes (at the regular price).
- A timelapse of each card's screenshots, and a month calendar to pick a day.
- App and site icons next to the names on cards, in Daily and in Weekly. Site icons are only looked up when you turn that on.
- Daily: a standup recap of the day (done, next, blockers), with a model of its own if you want, time per category, an activity grid and distractions. The recap can be written by itself each morning and saved as Markdown to a folder you pick, for example to read it on your phone.
- Weekly: charts of the week, a focus heat map by hour, and "Save as PNG…" on the charts and heat maps.
- Goals: a focus target and a distraction limit per day, and a checklist that Daytrace checks against what you did. Once a day, the first time you use the window on a new day, Daytrace shows how yesterday went and asks for today's targets; it starts after 3 days with recording, and Settings → Notifications turns it off.
- Each category can count as focus, as distraction or as neither; Daily, Weekly and Goals follow that choice. Their percentages of the day are shares of active time (recorded time without idle time); Idle shows its time without a share.
- Chat: ask about your days. Answers cite the cards they come from and can show a chart; a memory of your preferences, which you can edit, carries over between conversations. Chat also runs through the Claude Code CLI and the Codex CLI (Codex 0.122 or newer), with Daytrace's read tools only.
- Agents: start AgentPlayback to see your Claude Code and Codex sessions of the day, with their cost at API prices. It needs Node.js 20 or newer, runs on this PC and stays loaded while you switch sections; its links open in your browser.
- First-run setup, "What's new" after an update, a dot on Daily (and on the taskbar button) when a new recap is ready.

### Models
- Claude Code CLI and Codex CLI as providers: they use your Claude or ChatGPT subscription.
- A fallback provider takes over when the main one fails, and cards say which model wrote them.
- The title and summary instructions of the prompts can be edited in Settings.
- When analysis fails, the rail and a banner say why in plain words (a refused key, no credits, a usage limit, a signed-out CLI, a busy server) with the fix as a button.

### AI tools
- `daytrace-mcp` gives Claude Code, Claude Desktop and other MCP clients read access to your timeline, and works as a command line (`daytrace-mcp cli today`). The installer puts it on your PATH.
- One-click setup for Claude Code, Claude Desktop, Codex, Cursor and VS Code in Settings → AI tools, and a Claude Code plugin with skills for a standup, a weekly review and questions about your day.
- "Allow edits" (off by default) lets these tools rename, recategorize and delete activities, add, change and remove categories, and set day goals, through the running app; every change is logged.

### Recording and the PC
- Pause for 15, 30 or 60 minutes, until tomorrow or until you resume, also from the tray, from a script (`daytrace.exe --pause 30`, `--resume`) or with `daytrace://pause?minutes=30` links. Links can only pause.
- Pick private apps from the ones you used. The tray icon shows whether Daytrace is recording, paused or failing.
- After 15 idle minutes the window goes back to today (days start at 4 AM, as before).
- Notifications when the daily recap is ready, once when the first cards after setup are ready, and for problems that need you; each kind can be turned off.
- Updates install by themselves while the PC is idle or locked, and Daytrace starts again in the tray. Turn this off in Settings → Updates and about to get a notification instead.
- Daily database backups with recovery, a storage panel with a size limit, and "Copy debug info" (API keys, your user folder and window titles left out).
- Screenshots are still deleted after the retention period, but the text descriptions of the screen and the token counts are kept, so chat and recaps can use older days.

### Fixed
- `daytrace-mcp cli` says "1 activity" and "1 match".
- Keyboard shortcuts work on every keyboard layout (Ctrl+1 on Belgian AZERTY).
- Menus no longer hide the rest of the window from screen readers while they are open.
- Requests through the Claude Code and Codex CLIs stay under the 90k input-token ceiling too, with fewer screenshots when needed.

### Security
- A `daytrace://pause` link can't end a longer pause that is on, and it no longer starts Daytrace: when Daytrace is not running, it quits at once.
- The AgentPlayback dashboard sits behind a proxy that checks the Host and Origin of each request, and npm can't read folders above Daytrace's data folder.
- An AI tool that reads its reply slowly no longer misses it.
- Updates move `daytrace-mcp.exe` aside under a new name each time, and the uninstaller removes the leftovers.
- Category names and descriptions from AI tools have a length limit, and the AI tools' edit log is trimmed.
- A settings change made outside the window (by an AI tool, for example) is no longer overwritten by a save from the window.

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
