# Development

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

## Contributing

Issues and pull requests are welcome. Before a pull request, run `cargo test --lib --bins` in `src-tauri` and `npm test && npm run build` in the repository root; CI runs the same checks and builds the Windows installer. UI changes follow the design contract in [`src/design/README.md`](src/design/README.md), and `npm run dev` opens every screen in a browser with sample data, no Windows needed.

[← Back to the README](../README.md)
