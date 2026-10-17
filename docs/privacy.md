# Privacy and data

Screenshots, the timeline database and logs stay in `%APPDATA%\dev.daytrace.app`. Screenshots are deleted after the retention period (7 days by default), and the logged model requests and responses lose their text then; timeline cards, the text descriptions of the screen (observations) and token counts are kept, and app and window names for a year. With a cloud provider, the sampled screenshots of each batch are sent to that provider; chat answers and recaps send text from your timeline (cards, screen notes, app and window names), never screenshots. With Ollama or LM Studio nothing leaves the machine.

The Agents section downloads AgentPlayback from npm on its first start and lets it fetch a price list from GitHub; its dashboard is served on 127.0.0.1 only and its data stays in the `agents` folder of the data folder. Daytrace serves the dashboard through a small proxy that answers only requests for 127.0.0.1 and refuses requests from other sites; while it runs, other accounts on the same PC can open it too. Links in it open in your browser.

App icons next to app names are read from the programs on your PC. Sites get a letter; with Settings → Appearance → "Look up site icons" on (off by default), Daytrace asks each website named on the timeline for its `https://<site>/favicon.ico` directly, without a third-party favicon service.

Every request is checked against a ceiling of 90k input tokens (Haiku bills 5× above 100k) and is not sent if it would exceed it. The Claude Code and Codex CLIs get the same check: when a batch's screenshots would not fit, fewer of them are sent, and when Daytrace asks the CLI to fix an answer it could not use, that request is not sent if it would go over the ceiling.

Known limit: Microsoft Store apps are reported under a shared host process, so they cannot be matched by the private-app list.

[← Back to the README](../README.md)
