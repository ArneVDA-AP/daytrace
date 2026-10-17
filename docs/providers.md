# Model providers

| Provider | Setting | Notes |
|---|---|---|
| Claude | Anthropic API, default `claude-haiku-5-5` | Defaults to low reasoning effort. Optional Batch API halves the price; cards then arrive a few minutes later. The key is stored in Windows Credential Manager. |
| Ollama | `http://localhost:11434` | Nothing leaves your PC. Use the Lightweight mode. |
| LM Studio | `http://localhost:1234` | Same as Ollama. |
| Other | Any OpenAI-compatible endpoint | OpenAI, OpenRouter, or Gemini's `/v1beta/openai` endpoint. |
| Claude Code CLI | `claude -p`, default `haiku` | Uses your Claude subscription. Screenshots go to the CLI as files in a temporary folder; it writes the cards in one step. Chat runs through it too, with Daytrace's read tools only. Needs Claude Code signed in (and Git for Windows). |
| Codex CLI | `codex exec`, default `gpt-6-luna` | Uses your ChatGPT subscription. Like the Claude Code CLI, it gets a batch's screenshots and writes the cards in one request. Chat runs through it too, with Daytrace's read tools only (Codex 0.122 or newer). |

A fallback provider (Settings → Model provider) takes over a step when the main provider fails, for example when the CLI is signed out or out of usage. The title and summary instructions of the Detailed and Lightweight prompts can be edited in Settings.

[← Back to the README](../README.md)
