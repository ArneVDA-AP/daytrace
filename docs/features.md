# Features in detail

## Install and uninstall

Daytrace runs on 64-bit Windows 10 and 11.

1. There is no published release yet. Build the installer as described in [Development](development.md#building); the installer then sits in `src-tauri/target/release/bundle/nsis`. `daytrace-mcp.exe` is part of it.
2. Run it. The installer is not code-signed yet, so Windows SmartScreen can show "Windows protected your PC": select **More info**, then **Run anyway**. Daytrace installs for your Windows account only, without administrator rights, in `%LOCALAPPDATA%\Daytrace`, and puts that folder on your PATH so `daytrace-mcp` works in a terminal.
3. Daytrace opens with a short setup: what it records and where that goes, the model provider (an API key, Ollama or LM Studio, or a signed-in Claude Code or Codex CLI), the apps it must never capture, and whether it starts with Windows.
4. **Start recording** ends the setup. The first timeline card appears about 15 minutes later. Closing the window keeps Daytrace recording in the notification area; pause, resume or quit it from the tray icon.

After the first release, Daytrace looks for new versions by itself (see [Updates](#updates)). To remove Daytrace: Windows **Settings** → **Apps** → **Installed apps** → Daytrace → **Uninstall**. With "Delete the application data" checked, the uninstaller also removes the screenshots, the timeline and the settings in `%APPDATA%\dev.daytrace.app`; otherwise they stay.

## Overview

- **Capture.** One JPEG every 10 seconds (configurable) of the monitor that holds the active window; the monitor under the mouse when there is no usable active window. Recording pauses while Windows is locked or the screensaver runs. When a password manager or another app on your private list is in front, no screenshot is taken; when it is visible behind other windows, its window is blanked.
- **Away from the PC.** After 2 minutes without keyboard or mouse input (configurable), Daytrace stores no images until you are back, unless an app keeps the screen on (a video or call). That time shows as Idle and costs no model calls.
- **Analysis.** Screenshots are grouped into 15-minute batches. Batches where you were away get an "Idle" card without calling a model. Everything else goes to the model you choose, in one of two modes:
  - *Detailed*: one request with 15 screenshots, then the cards of the last 45 minutes are rewritten as a whole. Use this with Claude or another large model.
  - *Lightweight*: each screenshot is described on its own, then grouped into one card that may be merged with the previous one. Use this with small local vision models such as `qwen3-vl:4b` in Ollama.

  When a batch fails, the rail and a banner say why (a refused key, no credits, a usage limit, a signed-out CLI, a busy server) with the fix; Daytrace tries again with the next batch.
- **Timeline.** A day view (days start at 4 AM) with a strip of the whole day, cards with summaries and the screenshots they were written from, and a Week view of seven days. Cards can be renamed, recategorized, deleted, rated and reprocessed; a timelapse plays a card's screenshots, and a review mode walks through the cards you have not rated yet.
- **Daily, Weekly, Goals.** Daily has a standup recap (done, next, blockers), time per category and distractions; the recap can be written each morning and saved as Markdown to a folder. Weekly has charts of the week and a focus heat map by hour, each savable as PNG. Goals sets a focus target and a distraction limit per day, with a checklist. Each category counts as focus, distraction or neither.
- **Chat.** Questions about your days, answered from the timeline with links to the cards, through your model provider: the Claude API, a local model, an OpenAI-compatible endpoint, the Claude Code CLI or the Codex CLI.
- **Agents.** Starts [AgentPlayback](https://github.com/JerryZLiu/AgentPlayback) (Dayflow's Agents tab, run with `npx`, a pinned version) and shows its dashboard of your Claude Code and Codex sessions in the window. Needs Node.js 20 or newer.
- **AI tools.** `daytrace-mcp` gives Claude Code and other MCP clients your timeline, also as a command line, and a Claude Code plugin adds skills (see [AI tools](ai-tools.md)).
- **Pause from outside.** Pause and resume from the tray, a script, Task Scheduler or a `daytrace://pause` link (see [Pause from a script](#pause-from-a-script)).
- **Updates.** A daily check on GitHub; updates install by themselves while the PC is idle or locked (see [Updates](#updates)).
- **Storage.** Retention (7 days by default) and an optional size limit for screenshots, a daily backup of the database (the last 7 are kept) with recovery when it is damaged, and export of any range of days as Markdown.

## Pause from a script

```
daytrace.exe --pause 30            # minutes, 1 to 1440
daytrace.exe --pause tomorrow      # until the day starts (4 AM)
daytrace.exe --pause indefinite    # until resumed
daytrace.exe --resume
daytrace-mcp cli pause 30          # the same through the MCP program (resume: daytrace-mcp cli resume)
```

A running Daytrace takes the switch over from the second launch; otherwise Daytrace starts in the tray with it. This works from Task Scheduler, a shortcut or a Claude Code hook. The installer also registers `daytrace://pause?minutes=30` links. Any web page can open such a link after a browser prompt, so links can only pause, and resuming stays on the command line. A link never shortens a pause that is on, and when Daytrace is not running it quits at once instead of starting to record.

## Updates

Daytrace checks GitHub for a new release once a day. With "Install updates automatically" on (Settings → Updates and about, the default), it downloads the installer, checks its signature and installs it once the PC has been idle for 10 minutes or is locked, then starts again in the tray. "What's new" shows the release notes once after the update.

[← Back to the README](../README.md)
