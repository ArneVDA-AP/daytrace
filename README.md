# Daytrace

An automatic timeline of your day on Windows. Daytrace takes a screenshot every few seconds, has a vision model describe what you were doing every 15 minutes, and turns that into a timeline of activity cards you can review, search by day and export.

Daytrace is an unofficial port of [Dayflow](https://github.com/JerryZLiu/Dayflow) by Jerry Liu, which is macOS-only. The analysis design, constants and prompt texts come from Dayflow (MIT); the app around them is a new implementation in Rust and Tauri. It is not affiliated with or endorsed by the Dayflow project.

## What it does

- **Capture.** One JPEG of the monitor under your mouse every 10 seconds (configurable). Recording pauses while Windows is locked or the screensaver runs. When a password manager or another app on your private list is in front, no screenshot is taken.
- **Analysis.** Screenshots are grouped into 15-minute batches. Batches where you were away get an "Idle" card without calling a model. Everything else goes to the model you choose, in one of two modes:
  - *Detailed*: one request with 15 screenshots, then the cards of the last 45 minutes are rewritten as a whole. Use this with Claude or another large model.
  - *Lightweight*: each screenshot is described on its own, then grouped into one card that may be merged with the previous one. Use this with small local vision models such as `qwen3-vl:4b` in Ollama.
- **Timeline.** A day view (days start at 4 AM) with a strip for the whole day, cards with summaries, the screenshots each card was written from, and a retry button for batches that failed.
- **Export.** Any range of days as Markdown.

## Models

| Provider | Setting | Notes |
|---|---|---|
| Claude | Anthropic API, default `claude-haiku-5-5` | Roughly $0.01 per recorded hour with Haiku. The key is stored in Windows Credential Manager. |
| Ollama | `http://localhost:11434` | Nothing leaves your PC. Use the Lightweight mode. |
| LM Studio | `http://localhost:1234` | Same as Ollama. |
| Other | Any OpenAI-compatible endpoint | OpenAI, OpenRouter, or Gemini's `/v1beta/openai` endpoint. |

## Privacy

Screenshots, the timeline database and logs stay in `%APPDATA%\dev.daytrace.app`. Screenshots are deleted after the retention period (7 days by default); cards are kept. With a cloud provider, the sampled screenshots of each batch are sent to that provider. With Ollama or LM Studio nothing leaves the machine.

Known limit: Daytrace skips the whole screenshot when a private app is in front, but it cannot blank out a private app's window that is visible behind another window.

## Building

Requirements: Rust (stable), Node 22, and on Windows the WebView2 runtime (preinstalled on Windows 10/11).

```
npm ci
npm run tauri build      # installer in src-tauri/target/release/bundle/nsis
npm run tauri dev        # development
```

Pushes to `main` build a Windows installer in GitHub Actions; download it from the run's artifacts. Tags `v*` attach it to a draft release.

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
| `src/` | React UI |

## License

MIT. See [LICENSE](LICENSE). Includes analysis design and prompt texts from Dayflow, © 2025 Jerry Liu, MIT.
