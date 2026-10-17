---
name: setup
description: Check that Daytrace is installed and recording, then summarize the user's day. Use when the user asks to set up Daytrace or to check that the Daytrace plugin works.
allowed-tools: mcp__plugin_daytrace_daytrace__get_status mcp__plugin_daytrace_daytrace__get_timeline
---

<!-- Ported from Dayflow's plugins/dayflow/skills/onboarding/SKILL.md (MIT, (c) 2025 Jerry Liu). -->

# Set up Daytrace

Call `get_status` with `{}`.

- If the Daytrace tools are missing, Claude Code could not start
  `daytrace-mcp`. Daytrace's installer puts it on the user's PATH; tell the
  user to install Daytrace from https://github.com/ArneVDA-AP/daytrace/releases
  (or, if it is installed, to choose Add to PATH in Daytrace under Settings >
  AI tools), then restart Claude Code. Stop there.
- If the call fails because no data was found, Daytrace has not recorded yet.
  Tell the user to finish Daytrace's setup and let it record for about fifteen
  minutes. Stop there.
- If `last_capture_at` is more than an hour old, mention that recording looks
  paused and that they can resume it from Daytrace's icon in the Windows
  notification area.
- If `pending_batches` is above zero, mention that recent time is still being
  analyzed and will appear shortly.

Then call `get_timeline` with `{}` and give the user a three or four line
summary of their day so far, so they see it working. Tell them what they can
ask for next: where their time went, a standup, or a weekly review.

If `edits_enabled` is false, mention once that this plugin can only read the
timeline until they turn on "Allow edits" in Daytrace under Settings > AI tools.
If they installed the plugin during an existing task, continue that task.
