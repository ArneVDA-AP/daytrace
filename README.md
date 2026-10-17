# Daytrace

An automatic timeline of your day on Windows. Daytrace takes a screenshot every few seconds, has a vision model describe what you were doing every 15 minutes, and turns that into a timeline of activity cards you can review, search by day and export.

Daytrace is an unofficial port of [Dayflow](https://github.com/JerryZLiu/Dayflow) by Jerry Liu, which is macOS-only. The analysis design, constants and prompt texts come from Dayflow (MIT); the app around them is a new implementation in Rust and Tauri. It is not affiliated with or endorsed by the Dayflow project.

## What it does

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
- **AI tools.** `daytrace-mcp` gives Claude Code and other MCP clients your timeline, also as a command line, and a Claude Code plugin adds skills (see [AI tools](#ai-tools)).
- **Pause from outside.** Pause and resume from the tray, a script, Task Scheduler or a `daytrace://pause` link (see [Pause from a script](#pause-from-a-script)).
- **Updates.** A daily check on GitHub; updates install by themselves while the PC is idle or locked (see [Updates](#updates)).
- **Storage.** Retention (7 days by default) and an optional size limit for screenshots, a daily backup of the database (the last 7 are kept) with recovery when it is damaged, and export of any range of days as Markdown.

## Install

Daytrace runs on 64-bit Windows 10 and 11.

1. Download `Daytrace_<version>_x64-setup.exe` from the [latest release](https://github.com/ArneVDA-AP/daytrace/releases/latest). `daytrace-mcp.exe` is part of it.
2. Run it. The installer is not code-signed yet, so Windows SmartScreen can show "Windows protected your PC": select **More info**, then **Run anyway**. Daytrace installs for your Windows account only, without administrator rights, in `%LOCALAPPDATA%\Daytrace`, and puts that folder on your PATH so `daytrace-mcp` works in a terminal.
3. Daytrace opens with a short setup: what it records and where that goes, the model provider (an API key, Ollama or LM Studio, or a signed-in Claude Code or Codex CLI), the apps it must never capture, and whether it starts with Windows.
4. **Start recording** ends the setup. The first timeline card appears about 15 minutes later. Closing the window keeps Daytrace recording in the notification area; pause, resume or quit it from the tray icon.

After that Daytrace looks for new versions by itself (see [Updates](#updates)). To remove Daytrace: Windows **Settings** → **Apps** → **Installed apps** → Daytrace → **Uninstall**. With "Delete the application data" checked, the uninstaller also removes the screenshots, the timeline and the settings in `%APPDATA%\dev.daytrace.app`; otherwise they stay.

## Models

| Provider | Setting | Notes |
|---|---|---|
| Claude | Anthropic API, default `claude-haiku-5-5` | Defaults to low reasoning effort. Optional Batch API halves the price; cards then arrive a few minutes later. The key is stored in Windows Credential Manager. |
| Ollama | `http://localhost:11434` | Nothing leaves your PC. Use the Lightweight mode. |
| LM Studio | `http://localhost:1234` | Same as Ollama. |
| Other | Any OpenAI-compatible endpoint | OpenAI, OpenRouter, or Gemini's `/v1beta/openai` endpoint. |
| Claude Code CLI | `claude -p`, default `haiku` | Uses your Claude subscription. Screenshots go to the CLI as files in a temporary folder; it writes the cards in one step. Chat runs through it too, with Daytrace's read tools only. Needs Claude Code signed in (and Git for Windows). |
| Codex CLI | `codex exec`, default `gpt-6-luna` | Uses your ChatGPT subscription. Like the Claude Code CLI, it gets a batch's screenshots and writes the cards in one request. Chat runs through it too, with Daytrace's read tools only (Codex 0.122 or newer). |

A fallback provider (Settings → Model provider) takes over a step when the main provider fails, for example when the CLI is signed out or out of usage. The title and summary instructions of the Detailed and Lightweight prompts can be edited in Settings.

## AI tools

`daytrace-mcp.exe`, installed next to Daytrace, gives Claude Code, Claude Desktop and other MCP clients your timeline: activities, time per category, daily recaps and the screen notes behind each activity. The installer puts its folder on your PATH. Reading works while Daytrace is closed and never returns screenshots or the names of private apps.

- **Claude Code plugin** with skills for a standup, a weekly review and questions about your day. In Claude Code:
  ```
  /plugin marketplace add ArneVDA-AP/daytrace
  /plugin install daytrace@daytrace
  ```
- **One click** in Settings → AI tools adds `daytrace-mcp` to Claude Code (`claude mcp add --scope user`), Claude Desktop, Codex (`~/.codex/config.toml`), Cursor (`~/.cursor/mcp.json`) or VS Code (the default profile's `mcp.json`), each time after a backup copy of the file. Under "Set it up yourself" the same page has the command and the configuration to copy, and what any other MCP client needs: a local (stdio) server that runs `daytrace-mcp.exe` without arguments.
- **Terminal:** `daytrace-mcp cli today`, `daytrace-mcp help` for every command.
- **Allow edits** (Settings → AI tools, off by default) lets these tools rename, recategorize and delete activities, change categories and set day goals. Changes go through the running app over a named pipe that only your Windows account can open, and each one is logged in `agent-writes.log` in the data folder. Category names can be up to 60 characters and descriptions up to 500; `agent-writes.log` keeps entries for the retention period and at most 1 MB.

## Privacy

Screenshots, the timeline database and logs stay in `%APPDATA%\dev.daytrace.app`. Screenshots are deleted after the retention period (7 days by default), and the logged model requests and responses lose their text then; timeline cards, the text descriptions of the screen (observations) and token counts are kept, and app and window names for a year. With a cloud provider, the sampled screenshots of each batch are sent to that provider; chat answers and recaps send text from your timeline (cards, screen notes, app and window names), never screenshots. With Ollama or LM Studio nothing leaves the machine.

The Agents section downloads AgentPlayback from npm on its first start and lets it fetch a price list from GitHub; its dashboard is served on 127.0.0.1 only and its data stays in the `agents` folder of the data folder. Daytrace serves the dashboard through a small proxy that answers only requests for 127.0.0.1 and refuses requests from other sites; while it runs, other accounts on the same PC can open it too. Links in it open in your browser.

App icons next to app names are read from the programs on your PC. Sites get a letter; with Settings → Appearance → "Look up site icons" on (off by default), Daytrace asks each website named on the timeline for its `https://<site>/favicon.ico` directly, without a third-party favicon service.

Every request is checked against a ceiling of 90k input tokens (Haiku bills 5× above 100k) and is not sent if it would exceed it. The Claude Code and Codex CLIs get the same check: when a batch's screenshots would not fit, fewer of them are sent, and when Daytrace asks the CLI to fix an answer it could not use, that request is not sent if it would go over the ceiling.

Known limit: Microsoft Store apps are reported under a shared host process, so they cannot be matched by the private-app list.

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

## Building

Requirements: Rust (stable), Node 22, and on Windows the WebView2 runtime (preinstalled on Windows 10/11).

```
npm ci
npm run tauri build      # installer in src-tauri/target/release/bundle/nsis
npm run tauri dev        # development
```

Pushes to `main` build a Windows installer in GitHub Actions; download it from the run's artifacts (`daytrace-windows`, and `daytrace-mcp-windows` with `daytrace-mcp.exe` on its own). Tags `v*` publish a release with the installer, `Daytrace_<version>_x64-setup.exe`, its notes taken from the version's section of `CHANGELOG.md`.

Signed updates need the repository secret `TAURI_SIGNING_PRIVATE_KEY`: the existing private key whose public key is in `tauri.conf.json` > `plugins` > `updater` > `pubkey`, and, if that key has a password, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Installed copies only accept updates signed with that key. A new key (`npm run tauri signer generate -- -w <file>`) also needs its public key in `tauri.conf.json`, and copies installed before that change can then only be updated by running the new installer by hand. With the secret set, the release also gets a signed installer and `latest.json`, which "Install updates automatically" reads from `releases/latest/download/latest.json`. Without them the build still works; Daytrace then shows a notification and the release page for that release. Local builds (`npm run tauri build`) make no updater files.

### Testing the pipeline without waiting

`src-tauri/tests/replay.rs` feeds a folder of JPEG frames (10 seconds apart) through the real analysis pipeline:

```
cd src-tauri
DAYTRACE_API_KEY=... DAYTRACE_REPLAY_DIR=path/to/frames DAYTRACE_PIPELINE=cloud \
  cargo test --test replay -- --ignored --nocapture
```

## Layout

| Path | Contents |
|---|---|
| `src-tauri/src/capture.rs` | Screenshot loop, lock handling, private apps |
| `src-tauri/src/batching.rs` | 15-minute batching and the idle shortcut |
| `src-tauri/src/analysis.rs` | Analysis loop, card window rewrite, failure cards, retries, retention |
| `src-tauri/src/llm/` | Model clients (Anthropic, OpenAI-compatible, Claude Code and Codex CLI), both pipelines, prompts, fallback routing, failure causes |
| `src-tauri/src/db.rs` | SQLite schema and queries |
| `src-tauri/src/platform.rs` | Win32: idle time, lock state, foreground app, Credential Manager |
| `src-tauri/src/tray.rs` | Tray icon and menu, the main window |
| `src-tauri/src/features/` | Daily and Weekly (`insights`), Goals (`journal`), Chat (`chat`), card edits and reprocessing (`timeline_extras`), Agents (`agents.rs`), backups and storage (`storage`), updates, pause switches, notifications and debug info (`platform_extras`), app and site icons (`icons.rs`), the rail's badges and link opener (`shell.rs`) |
| `src-tauri/src/features/agent_bridge/` | `daytrace-mcp`: MCP server, command line, write bridge, PATH and client setup |
| `src-tauri/windows/` | NSIS installer hooks (PATH) |
| `src/app/`, `src/components/`, `src/design/` | React UI: shell, rail, banners, command menu; shared components; design contract and tokens (`src/design/README.md`) |
| `src/views/<section>/` | One folder per section (timeline, daily, weekly, journal = Goals, chat, agents, settings, onboarding) |
| `src/dev/` | Browser preview without Tauri: mocked commands with synthetic data (`npm run dev`) |
| `scripts/` | Screenshots, contrast check, build checks, release notes from `CHANGELOG.md` |
| `claude-plugin/`, `.claude-plugin/` | Claude Code plugin and its marketplace file |

Tests: `cargo test --lib --bins` in `src-tauri`, and `npm test` for the UI's pure modules (`src/**/*.test.mjs`). CI runs both on pushes to `main` and on pull requests.

## License

MIT. See [LICENSE](LICENSE). Includes analysis design and prompt texts from Dayflow, © 2025 Jerry Liu, MIT.
