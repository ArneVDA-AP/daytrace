---
name: standup
description: Draft a standup or daily status update from the user's Daytrace timeline. Use when the user asks for a standup, daily update, or "what did I do yesterday".
allowed-tools: mcp__plugin_daytrace_daytrace__get_daily mcp__plugin_daytrace_daytrace__get_timeline mcp__plugin_daytrace_daytrace__get_activity_detail mcp__plugin_daytrace_daytrace__get_status
---

<!-- Ported from Dayflow's plugins/dayflow/skills/standup/SKILL.md (MIT, (c) 2025 Jerry Liu). -->

# Standup from Daytrace

1. Work out which day the user means. "Yesterday" on a Monday usually means
   Friday; ask only if it is unclear.
2. Call `get_daily` for that date. If Daytrace already wrote a daily recap, use
   its highlights, tasks, and blockers as the starting point.
3. If there is no daily recap, call `get_timeline` for the day and build the
   update from the activities, skipping `Idle`, `System`, and distraction
   entries.
4. Call `get_activity_detail` for at most three activities where the one-line
   summary is too thin to describe the work.

Write the update as three short sections: what got done, what is next, and
blockers. Describe outcomes rather than apps and minutes, keep it to what a
teammate needs, and leave out anything personal. Say "None" for blockers
rather than inventing one.
