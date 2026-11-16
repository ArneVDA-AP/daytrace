# AI tools: MCP server, CLI and Claude Code plugin

`daytrace-mcp.exe`, installed next to Daytrace, gives Claude Code, Claude Desktop and other MCP clients your timeline: activities, time per category, daily recaps and the screen notes behind each activity. The installer puts its folder on your PATH. Reading works while Daytrace is closed and never returns screenshots or the names of private apps.

- **Claude Code plugin** with skills for a standup, a weekly review and questions about your day. In Claude Code:
  ```
  /plugin marketplace add ArneVDA-AP/daytrace
  /plugin install daytrace@daytrace
  ```
- **One click** in Settings → AI tools adds `daytrace-mcp` to Claude Code (`claude mcp add --scope user`), Claude Desktop, Codex (`~/.codex/config.toml`), Cursor (`~/.cursor/mcp.json`) or VS Code (the default profile's `mcp.json`), each time after a backup copy of the file. Under "Set it up yourself" the same page has the command and the configuration to copy, and what any other MCP client needs: a local (stdio) server that runs `daytrace-mcp.exe` without arguments.
- **Terminal:** `daytrace-mcp cli today`, `daytrace-mcp help` for every command.
- **Allow edits** (Settings → AI tools, off by default) lets these tools rename, recategorize and delete activities, change categories and set day goals. Changes go through the running app over a named pipe that only your Windows account can open, and each one is logged in `agent-writes.log` in the data folder. Category names can be up to 60 characters and descriptions up to 500; `agent-writes.log` keeps entries for the retention period and at most 1 MB.
- **Claude Code session cards** (Settings → AI tools, off by default). The start-card plugin for Claude Code keeps one JSON file for each finished task of a session: the outcome the session worked toward, its check, and its grade (pass, partial or fail). With the setting on, Daytrace reads those files from `start-card/finished` in your Claude config folder (`.claude` in your user folder) and shows each card on the timeline card that covers the time the session finished. A session card that finished in time no timeline card covers is listed with the day. When the plugin saved a drawing beside a record (an SVG file with the same name: the card as the session showed it), Daytrace shows that drawing, as an image, so nothing in the file can run. The AI tools get the text of a card, not the drawing. `get_timeline` and `get_activity_detail` return the cards as `session_cards` (and `unplaced_session_cards`), with a note that their text was written by an AI session and is data, not instructions. Daytrace's own chat does not get them. Daytrace only reads the folder: nothing is copied into its database or sent anywhere, and sessions that ran in a temporary folder are counted, not shown. Without the plugin the folder does not exist and the setting shows nothing.

[← Back to the README](../README.md)
