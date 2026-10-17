# Daytrace

An automatic timeline of your day on Windows. Daytrace takes a screenshot every few seconds, has a vision model describe what you were doing every 15 minutes, and turns that into a timeline of activity cards you can review, search by day and export.

Daytrace is an unofficial port of [Dayflow](https://github.com/JerryZLiu/Dayflow) by Jerry Liu, which is macOS-only. The analysis design, constants and prompt texts come from Dayflow (MIT); the app around them is a new implementation in Rust and Tauri. It is not affiliated with or endorsed by the Dayflow project.

## What it does

- **Capture.** One JPEG every 10 seconds (configurable) of the monitor that holds the active window; the monitor under the mouse when there is no usable active window. Recording pauses while Windows is locked or the screensaver runs. When a password manager or another app on your private list is in front, no screenshot is taken; when it is visible behind other windows, its window is blanked.
- **Away from the PC.** After 2 minutes without keyboard or mouse input (configurable), Daytrace stores no images until you are back, unless an app keeps the screen on (a video or call). That time shows as Idle and costs no model calls.
- **Analysis.** Screenshots are grouped into 15-minute batches. Batches where you were away get an "Idle" card without calling a model. Everything else goes to the model you choose, in one of two modes:
  - *Detailed*: one request with 15 screenshots, then the cards of the last 45 minutes are rewritten as a whole. Use this with Claude or another large model.
  - *Lightweight*: each screenshot is described on its own, then grouped into one card that may be merged with the previous one. Use this with small local vision models such as `qwen3-vl:4b` in Ollama.
- **Timeline.** A day view (days start at 4 AM) with a strip for the whole day, cards with summaries, the screenshots each card was written from, and a retry button for batches that failed.
- **Export.** Any range of days as Markdown.

## Models

| Provider | Setting | Notes |
|---|---|---|
| Claude | Anthropic API, default `claude-haiku-5-5` | Defaults to low reasoning effort. Optional Batch API halves the price; cards then arrive a few minutes later. The key is stored in Windows Credential Manager. |
| Ollama | `http://localhost:11434` | Nothing leaves your PC. Use the Lightweight mode. |
| LM Studio | `http://localhost:1234` | Same as Ollama. |
| Other | Any OpenAI-compatible endpoint | OpenAI, OpenRouter, or Gemini's `/v1beta/openai` endpoint. |
| Claude Code CLI | `claude -p`, default `haiku` | Uses your Claude subscription. Screenshots go to the CLI as files in a temporary folder; it writes the cards in one step. Chat runs through it too, with Daytrace's read tools only. Needs Claude Code signed in (and Git for Windows). |
| Codex CLI | `codex exec`, default `gpt-6-luna` | Uses your ChatGPT account, same one-step mode. |

A fallback provider (Settings → Model provider) takes over a step when the main provider fails, for example when the CLI is signed out or out of usage. The title and summary instructions of the Detailed and Lightweight prompts can be edited in Settings.

## AI tools

`daytrace-mcp.exe`, installed next to Daytrace, gives Claude Code, Claude Desktop and other MCP clients your timeline: activities, time per category, daily recaps and the screen notes behind each activity. The installer puts its folder on your PATH. Reading works while Daytrace is closed and never returns screenshots or the names of private apps.

- **Claude Code plugin** with skills for a standup, a weekly review and questions about your day. In Claude Code:
  ```
  /plugin marketplace add ArneVDA-AP/daytrace
  /plugin install daytrace@daytrace
  ```
- **One click** in Settings → AI tools adds `daytrace-mcp` to Claude Code (`claude mcp add --scope user`) or Claude Desktop (its config file, after a backup copy). The same page has the setup to copy for other clients.
- **Terminal:** `daytrace-mcp cli today`, `daytrace-mcp help` for every command.
- **Allow edits** (Settings → AI tools, off by default) lets these tools rename, recategorize and delete activities, change categories and set day goals. Changes go through the running app over a named pipe that only your Windows account can open, and each one is logged in `agent-writes.log` in the data folder.

## Privacy

Screenshots, the timeline database and logs stay in `%APPDATA%\dev.daytrace.app`. Screenshots are deleted after the retention period (7 days by default), and the logged model requests and responses lose their text then; timeline cards, the text descriptions of the screen (observations) and token counts are kept, and app and window names for a year. With a cloud provider, the sampled screenshots of each batch are sent to that provider. With Ollama or LM Studio nothing leaves the machine.

Every request is checked against a ceiling of 90k input tokens (Haiku bills 5× above 100k) and is not sent if it would exceed it. This includes the Claude Code and Codex one-step path, which sends fewer screenshots when they would not fit and stops correcting a card once the session would pass the ceiling.

Known limit: Microsoft Store apps are reported under a shared host process, so they cannot be matched by the private-app list.

## Pause from a script

```
daytrace.exe --pause 30            # minutes, 1 to 1440
daytrace.exe --pause tomorrow      # until the day starts (4 AM)
daytrace.exe --pause indefinite    # until resumed
daytrace.exe --resume
daytrace-mcp cli pause 30          # the same through the MCP program (resume: daytrace-mcp cli resume)
```

A running Daytrace takes the switch over from the second launch; otherwise Daytrace starts in the tray with it. This works from Task Scheduler, a shortcut or a Claude Code hook. The installer also registers `daytrace://pause?minutes=30` links. Any web page can open such a link after a browser prompt, so links can only pause, and resuming stays on the command line.

## Updates

Daytrace checks GitHub for a new release once a day. With "Install updates automatically" on (Settings → Updates and about, the default), it downloads the installer, checks its signature and installs it once the PC has been idle for 10 minutes or is locked, then starts again in the tray. "What's new" shows the release notes once after the update.

## Building

Requirements: Rust (stable), Node 22, and on Windows the WebView2 runtime (preinstalled on Windows 10/11).

```
npm ci
npm run tauri build      # installer in src-tauri/target/release/bundle/nsis
npm run tauri dev        # development
```

Pushes to `main` build a Windows installer in GitHub Actions; download it from the run's artifacts. Tags `v*` publish a release with the installer, its notes taken from the version's section of `CHANGELOG.md`.

Signed updates need the repository secret `TAURI_SIGNING_PRIVATE_KEY` (the private key made by `npm run tauri signer generate -- -w <file>`, whose public key is in `tauri.conf.json` > `plugins` > `updater` > `pubkey`) and, if the key has a password, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. With them the release also gets a signed installer and `latest.json`, which "Install updates automatically" reads from `releases/latest/download/latest.json`. Without them the build still works; Daytrace then shows a notification and the release page for that release. Local builds (`npm run tauri build`) make no updater files.

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
| `src-tauri/src/llm/` | Model client (Anthropic and OpenAI-compatible), both pipelines, prompts |
| `src-tauri/src/db.rs` | SQLite schema and queries |
| `src-tauri/src/platform.rs` | Win32: idle time, lock state, foreground app, Credential Manager |
| `src-tauri/src/features/agent_bridge/` | `daytrace-mcp`: MCP server, command line, write bridge, PATH and client setup |
| `claude-plugin/`, `.claude-plugin/` | Claude Code plugin and its marketplace file |
| `src/` | React UI |

## License

MIT. See [LICENSE](LICENSE). Includes analysis design and prompt texts from Dayflow, © 2025 Jerry Liu, MIT.
