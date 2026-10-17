---
name: weekly-review
description: Review a week of the user's time from Daytrace - where it went, how it compares, and what stands out. Use for weekly reviews, retros, and "how did my week go".
allowed-tools: mcp__plugin_daytrace_daytrace__get_weekly mcp__plugin_daytrace_daytrace__get_timeline mcp__plugin_daytrace_daytrace__get_time_breakdown mcp__plugin_daytrace_daytrace__get_activity_detail
---

<!-- Ported from Dayflow's plugins/dayflow/skills/weekly-review/SKILL.md (MIT, (c) 2025 Jerry Liu). -->

# Weekly review from Daytrace

1. Call `get_weekly` for the week in question (pass any date inside it; weeks
   start Monday at 4 AM). Omit the date for the current week.
2. Call `get_weekly` for the week before, so there is something to compare to.
3. For the two or three days that look most interesting, call `get_timeline`
   to see what the time was actually spent on.

Then write a short review:

- Total tracked time and the split by category, with the change from the
  previous week where it is meaningful.
- The main things the user worked on, grouped by project rather than by day.
- One or two observations worth acting on, such as a day lost to context
  switching or a project that got less time than the user seems to want.

Stay with what the data shows. Do not score the week or moralize about
distraction time; report it plainly and let the user decide what it means.
