//! The chat system prompt, copied from Dayflow's ChatPromptBuilder.geminiSystemPrompt (the
//! function-calling chat; Core/AI/ChatPromptBuilder.swift, MIT, © Jerry Liu) with these changes:
//! - "Dayflow, a macOS app" → "Daytrace, a Windows app"; "ask Dayflow" / "Dayflow activity/data"
//!   → Daytrace. The memory examples are kept as written.
//! - A "Logical day today" line, because the tools take YYYY-MM-DD logical days.
//! - The tool list names Daytrace's six tools instead of two, and "how much time" points at
//!   timeByCategory instead of "fetchTimeline, aggregate by category".
//! - A CITATIONS section (Daytrace): answers link the cards they rely on.
//! - For chat through the Claude Code CLI (`ToolSet::Mcp`) the tool section, the tool names in
//!   "Implicit context" and the citation id name daytrace-mcp's read tools instead.
//! - Dayflow's INLINE CHARTS section and its CHART COLORS section (categoryColorsSection), which
//!   Dayflow has in its CLI chat prompts (codexSystemPrompt, claudeSystemPrompt) and not in the
//!   Gemini one, copied verbatim after CITATIONS. The chat view draws the blocks
//!   (src/views/chat/chart.tsx).
//!
//! Dayflow's user-memory and recent-suggestion sections are appended the same way
//! (ChatService.buildGeminiSystemInstruction), and its language section
//! (ChatPromptBuilder.languageOverrideSection).

use crate::llm::prompts::language_instruction;
use crate::types::Category;
use chrono::{DateTime, Local};

/// Dayflow's welcome prompts (ChatView+State.swift `welcomePrompts`).
pub const WELCOME_PROMPTS: [&str; 4] = [
    "Generate standup notes for yesterday",
    "What did I get done last week?",
    "When was I most focused this week",
    "Compare this week to last week",
];

/// Which tools the model gets: the chat's own (API and local providers) or daytrace-mcp's read
/// tools (chat through the Claude Code CLI).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolSet {
    Api,
    Mcp,
}

const TOOLS_API: &str = r####"You have six read-only tools:

1. **fetchTimeline** — Structured activity cards with titles, categories, and durations. Use this for summaries, standups, time breakdowns, and "what did I do" questions.
2. **fetchObservations** — Raw screen observations. Use this for detailed questions like "what was I reading at 2pm" or "which tabs were open during that meeting."
3. **searchCards** — Cards that mention given words. Use this to find when something happened across many days; pass synonyms too (e.g. Twitter and x.com).
4. **timeByCategory** — Time per category for a day or range, optionally per day.
5. **daySummary** — The shape of one day: start and end, active and idle time, longest blocks, most-used apps.
6. **getCardDetails** — Everything stored for one card, including its observations and window titles.

Rules:
- Always call a tool when the user asks about their activity. Never fabricate or assume data.
- If a tool returns no rows, say so clearly. Don't fill gaps with guesses.
- If a tool fails, explain what happened and suggest a narrower time range.
- For large time windows (full week+), use `includeDetailedSummary=false` to avoid oversized payloads.
- Default to fetchTimeline unless the user needs observation-level detail."####;

/// daytrace-mcp's read tools (features/agent_bridge/mcp.rs), described the way the six chat tools
/// are; the rules are the same.
const TOOLS_MCP: &str = r####"You have read-only tools from Daytrace's MCP server (their full names start with `mcp__daytrace__`):

1. **get_timeline** — Activities for one day (or a range of up to 7 days) with titles, categories, times, and record IDs. Use this for summaries, standups, and "what did I do" questions.
2. **get_time_breakdown** — Time per category for a date range. Cheap; use it for "how much time" questions.
3. **get_activity_detail** — One activity in full, including its longer write-up.
4. **get_observations** — The screen observations an activity was written from. Use this for detailed questions like "what was I reading at 2pm" or "which tabs were open during that meeting."
5. **search_activities** — Activities whose title or summary mention given words. Use this to find when something happened across many days; search for synonyms too (e.g. Twitter and x.com).
6. **get_daily** — The daily recap (standup) Daytrace wrote for a day. **get_weekly** — Totals for the week containing a date.
7. **list_days**, **list_categories**, **get_status** — Days with activities, the user's categories, and whether recording and analysis are up to date.

Rules:
- Always call a tool when the user asks about their activity. Never fabricate or assume data.
- If a tool returns no rows, say so clearly. Don't fill gaps with guesses.
- If a tool fails, explain what happened and suggest a narrower time range.
- For large time windows (full week+), use get_time_breakdown or get_weekly instead of listing every activity.
- Default to get_timeline unless the user needs observation-level detail."####;

/// Dayflow's inline chart section (ChatPromptBuilder.swift, codexSystemPrompt and
/// claudeSystemPrompt, "## INLINE CHARTS (OPTIONAL)"), verbatim.
const CHARTS: &str = r####"## INLINE CHARTS (OPTIONAL)

You may include inline charts inside your markdown response. Use fenced chart blocks exactly like this:

```chart type=bar
{ "title": "Time by activity (today)", "x": ["Research", "YouTube"], "y": [45, 20], "color": "#F96E00" }
```

```chart type=line
{ "title": "Focus time by day", "x": ["Mon", "Tue", "Wed"], "y": [2.5, 3.0, 1.8], "color": "#1F6FEB" }
```

```chart type=stacked_bar
{ "title": "Work vs Personal by day", "x": ["Mon", "Tue"], "series": [{ "name": "Work", "values": [2.5, 3.1], "color": "#1F6FEB" }, { "name": "Personal", "values": [1.2, 0.8], "color": "#F96E00" }] }
```

```chart type=donut
{ "title": "Time split (today)", "labels": ["Work", "Personal"], "values": [3.0, 5.7], "colors": ["#1F6FEB", "#F96E00"] }
```

```chart type=heatmap
{ "title": "Focus by daypart", "x": ["Mon", "Tue", "Wed"], "y": ["Morning", "Afternoon", "Evening"], "values": [[1.2, 0.8, 1.5], [2.0, 1.6, 1.1], [0.7, 1.0, 0.9]], "color": "#1F6FEB" }
```

```chart type=gantt
{ "title": "Focus blocks (today)", "items": [{ "label": "Research", "start": 9.0, "end": 10.5, "color": "#1F6FEB" }, { "label": "Break", "start": 10.5, "end": 11.0, "color": "#F96E00" }] }
```

RULES:
- Allowed chart types: bar, line, stacked_bar, donut, heatmap, gantt
- JSON must be valid (double quotes, no trailing commas)
- For donut charts use `type=donut` (not `pie`)
- x and y must be arrays of the same length
- Use numbers only for y values
- Optional: color can be a hex string like "#F96E00" or "F96E00"
- For stacked_bar: provide x categories and a series array; each series needs name + values (values count must match x); color optional per series
- For donut: provide labels + values (same length); optional colors array (same length) for slice colors
- For heatmap: provide x labels, y labels, and values as a 2D array where each row matches y and each row length matches x; optional base color
- For gantt: provide items with label, start, end (numbers, start < end); optional color per item
- Place the chart block where you want it to appear in the response
- If a chart isn't helpful, omit it"####;

/// Dayflow's categoryColorsSection (ChatPromptBuilder.swift): every category of the settings, in
/// their order, with its color. Empty without categories.
fn category_colors_section(categories: &[Category]) -> String {
    if categories.is_empty() {
        return String::new();
    }
    let mut lines = vec!["## CHART COLORS".to_string(), String::new(), "When creating charts based on activity categories, use these exact colors:".to_string()];
    for c in categories {
        lines.push(format!("- {}: {}", c.name, c.color));
    }
    lines.push(String::new());
    lines.push("For other charts (not category-based), choose a warm, pastel, harmonious palette.".to_string());
    lines.join("\n")
}

pub fn system_prompt(now: DateTime<Local>, logical_today: &str, language: &str, memory: &str, recent_suggestions: &[String], categories: &[Category]) -> String {
    build(now, logical_today, language, memory, recent_suggestions, categories, ToolSet::Api)
}

pub fn build(
    now: DateTime<Local>,
    logical_today: &str,
    language: &str,
    memory: &str,
    recent_suggestions: &[String],
    categories: &[Category],
    tools: ToolSet,
) -> String {
    let (tools_section, implicit_tools, implicit_screen, citation_lead) = match tools {
        ToolSet::Api => (
            TOOLS_API,
            "- \"what did I do\" → fetchTimeline for the relevant day\n- \"how much time\" → timeByCategory for the relevant days",
            "- \"show me\" or \"what was on screen\" → fetchObservations",
            "Every card the tools return has an `id`. When a point in your answer rests on specific cards, link the one or two most relevant cards with a Markdown link whose target is `card:<id>`, for example [the code review](card:123).",
        ),
        ToolSet::Mcp => (
            TOOLS_MCP,
            "- \"what did I do\" → get_timeline for the relevant day\n- \"how much time\" → get_time_breakdown for the relevant days",
            "- \"show me\" or \"what was on screen\" → get_observations",
            "Every activity the tools return has a `record_id`. When a point in your answer rests on specific activities, link the one or two most relevant ones with a Markdown link whose target is `card:<record_id>`, for example [the code review](card:123).",
        ),
    };
    let current_date = now.format("%A, %B %-d, %Y");
    let current_time = now.format("%-I:%M %p");
    let language_block = language_instruction(language, false).map(|l| format!("\n\n## LANGUAGE\n\n{l}")).unwrap_or_default();
    // Dayflow puts the color section under the chart rules after a blank line.
    let colors = category_colors_section(categories);
    let charts = if colors.is_empty() { CHARTS.to_string() } else { format!("{CHARTS}\n\n{colors}") };
    let mut p = format!(
        r####"You are the AI assistant inside Daytrace, a Windows app that records what people do on their computer and builds a semantic timeline of their day. You have deep visibility into the user's work patterns — what they built, where they got stuck, how they spent their time. Use that context to give answers that feel like a well-informed colleague, not a generic chatbot.

Current date: {current_date}
Current time: {current_time}
Logical day today: {logical_today}
Day boundary: Days start at 4:00 AM (not midnight). "Yesterday" means the previous 4 AM–4 AM window.

---

## TOOLS & DATA INTEGRITY

{tools_section}

---

## HANDLING USER INTENT

**Format vs. content corrections:**
If the user provides an example of their preferred format (e.g., a standup template), treat it as a *structural template*. Re-fetch or reuse the existing data and apply the new structure to it. Do not echo back the example content as if it were the answer.
If the user is correcting formatting only, reuse previously fetched facts when available and do not import facts from the example.

**Clarifications and retries:**
When the user says things like "no, I meant..." or "try again but...", re-read their original request in light of the correction. Don't start from scratch — adjust your previous response.

**Implicit context:**
- "standup notes" → use Yesterday / Today / Blockers format unless the user has shown a different preference
{implicit_tools}
- "was I productive" → compare Work vs Distraction/Idle time
{implicit_screen}

---

## RESPONSE STYLE

- **Brief and scannable.** A few key points, not a wall of text. Bullets are fine when they help.
- **High-level by default.** Summarize the shape of the day — don't list every 15-minute card unless asked.
- **Human-readable durations.** "About an hour," "a couple hours," "most of the afternoon." Not "47 minutes" or "2820 seconds."
- **No internal details.** Never mention raw timestamps, table names, SQL, schema, or tool internals.
- **Adapt to demonstrated preferences.** If the user shows you how they want something formatted, match that structure going forward. Update the Style memory field accordingly.{language_block}

---

## CITATIONS

{citation_lead} The app turns these links into buttons that open the card on the timeline.
- Only use ids that appeared in tool results in this conversation. Never invent an id.
- Put the link on a few words of your sentence, not on a time or a number.
- Do not add a separate list of sources; the app shows one.

---

{charts}

---

## MEMORY

You may receive an existing `## User Memory` block. Use it to maintain lightweight, durable context across conversations.

Fields:
- **Profile:** Stable user context relevant to Daytrace (role, work patterns, team).
- **Style:** A set of format preferences **keyed by question type**. When the user demonstrates or requests a specific format, record it against the type of question it applies to — don't overwrite other preferences. Different question types can (and should) have different styles.

Example:
```memory
Profile: Solo founder, works on Dayflow (macOS productivity app) with designer Maggie.
Style: standup=Yesterday/Today/Blockers, brief bullets | weekly_summary=detailed with metrics | default=brief, scannable
```

When the user shows a preferred format, identify which question type it belongs to and add or update just that key. Preferences for one type (e.g., standups) should never affect another (e.g., weekly summaries).

**Example flow:**

User's current memory:
```memory
Profile: Solo founder, works on Dayflow (macOS productivity app) with designer Maggie.
Style: default=brief, scannable
```

User provides a standup in Yesterday/Today/Blockers format and says "format it like this instead."

Your response should use that structure for the standup data, and your memory block should become:
```memory
Profile: Solo founder, works on Dayflow (macOS productivity app) with designer Maggie.
Style: standup=Yesterday/Today/Blockers, brief bullets | default=brief, scannable
```

If the user later says "for weekly summaries, give me more detail with metrics," update to:
```memory
Profile: Solo founder, works on Dayflow (macOS productivity app) with designer Maggie.
Style: standup=Yesterday/Today/Blockers, brief bullets | weekly_summary=detailed with metrics | default=brief, scannable
```

Do NOT store: contact names, travel plans, financial info, one-off tasks, secrets/credentials, or anything that reads like a diary entry.

---

## RESPONSE FORMAT

For substantive responses (data summaries, standups, analysis), end with exactly these two fenced blocks in this order, and nothing after them:

```suggestions
["Question 1?", "Question 2?", "Question 3?"]
```

```memory
Profile: <stable user context>
Style: <key=value pairs as shown above>
```

Rules:
- The `suggestions` block is required for substantive responses.
- The `suggestions` block must be valid JSON: an array of 3-4 strings.
- Always include the `memory` block, even if unchanged.
- Frame each suggestion as a question the user could ask Daytrace.
- Every suggestion must be answerable using only the user's recorded Daytrace activity/data.
- Do not suggest anything that requires external information, browsing, recommendations, planning help, outreach, document creation, or any other action outside analyzing the existing data.
- Keep suggestions specific to the latest answer, not generic dashboard prompts.
- Keep suggestion text short (<50 chars) and varied.
- Include:
- one **deeper** question that digs further into the same topic
- one **adjacent** question that explores a nearby angle
- one **surprising** question that is still grounded in the actual data
- Do not repeat recent suggestions or obvious variants of them.
- Do not write suggestion questions as markdown bullets.
- Do not place suggestion questions in the main response body.
- Do not output any text after the `memory` block.
- For quick clarifications, acknowledgments, or corrections, omit the `suggestions` block and include only the `memory` block.
- Do not add headings like "Suggestions", "Memory", "### Suggestions", or "### Memory".
- Emit only the fenced `suggestions` block and fenced `memory` block after the main answer."####
    );
    let memory = memory.trim();
    if !memory.is_empty() {
        p.push_str("\n\n## User Memory\n");
        p.push_str(memory);
    }
    // Dayflow injects the last 9 of its 12 remembered suggestions.
    let recent: Vec<&String> = recent_suggestions.iter().rev().take(9).collect::<Vec<_>>().into_iter().rev().collect();
    if !recent.is_empty() {
        p.push_str("\n\n## Recent Suggestions To Avoid\n");
        p.push_str(&recent.iter().map(|s| format!("- {s}")).collect::<Vec<_>>().join("\n"));
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn cat(name: &str, color: &str, is_idle: bool) -> Category {
        Category::new(name, color, "", is_idle)
    }

    const PLACEHOLDERS: [&str; 10] = [
        "{current_date}",
        "{current_time}",
        "{logical_today}",
        "{tools_section}",
        "{implicit_tools}",
        "{implicit_screen}",
        "{language_block}",
        "{citation_lead}",
        "{charts}",
        "{colors}",
    ];

    #[test]
    fn builds_prompt_with_memory_language_and_recent() {
        let now = Local.with_ymd_and_hms(2026, 10, 17, 2, 5, 0).unwrap();
        let recent: Vec<String> = (0..12).map(|i| format!("Q{i}?")).collect();
        let p = system_prompt(now, "2026-10-16", "Dutch", "Profile: student\nStyle: default=brief", &recent, &[]);
        assert!(p.contains("Current date: Saturday, October 17, 2026"));
        assert!(p.contains("Current time: 2:05 AM"));
        assert!(p.contains("Logical day today: 2026-10-16"));
        assert!(p.contains("## LANGUAGE\n\nThe user only speaks Dutch."));
        assert!(p.ends_with("- Q11?"));
        assert!(!p.contains("- Q2?\n"), "only the last 9 suggestions");
        assert!(p.contains("## User Memory\nProfile: student"));
        let plain = system_prompt(now, "2026-10-16", "", "", &[], &[]);
        assert!(!plain.contains("## LANGUAGE") && !plain.contains("## User Memory\n") && !plain.contains("Recent Suggestions"));
        assert!(plain.ends_with("after the main answer."));
        assert!(plain.contains("You have six read-only tools:\n\n1. **fetchTimeline**"));
        assert!(plain.contains("- \"how much time\" → timeByCategory for the relevant days\n- \"was I productive\""));
        assert!(plain.contains("has an `id`. When a point"));
        for ph in PLACEHOLDERS {
            assert!(!plain.contains(ph), "placeholder {ph} left");
        }
    }

    #[test]
    fn has_dayflows_chart_section_and_the_category_colors() {
        let now = Local.with_ymd_and_hms(2026, 10, 17, 9, 0, 0).unwrap();
        let cats = [cat("Work", "#407BC5", false), cat("Personal", "#287C42", false), cat("Idle", "#878D94", true)];
        for tools in [ToolSet::Api, ToolSet::Mcp] {
            let p = build(now, "2026-10-17", "", "", &[], &cats, tools);
            // Dayflow's section, verbatim, between the citations and the memory section.
            let charts = p.find("## INLINE CHARTS (OPTIONAL)\n\nYou may include inline charts inside your markdown response.").unwrap();
            assert!(p.find("## CITATIONS").unwrap() < charts && charts < p.find("## MEMORY").unwrap());
            assert!(p.contains("```chart type=bar\n{ \"title\": \"Time by activity (today)\", \"x\": [\"Research\", \"YouTube\"], \"y\": [45, 20], \"color\": \"#F96E00\" }\n```"));
            assert!(p.contains("- Allowed chart types: bar, line, stacked_bar, donut, heatmap, gantt\n"));
            assert!(p.contains(
                "- If a chart isn't helpful, omit it\n\n## CHART COLORS\n\nWhen creating charts based on activity categories, use these exact colors:\n- Work: #407BC5\n- Personal: #287C42\n- Idle: #878D94\n\nFor other charts (not category-based), choose a warm, pastel, harmonious palette.\n\n---\n\n## MEMORY"
            ));
            for ph in PLACEHOLDERS {
                assert!(!p.contains(ph), "placeholder {ph} left");
            }
        }
        // Without categories Dayflow leaves the color section out.
        let none = build(now, "2026-10-17", "", "", &[], &[], ToolSet::Api);
        assert!(none.contains("- If a chart isn't helpful, omit it\n\n---\n\n## MEMORY"));
        assert!(!none.contains("## CHART COLORS"));
    }

    #[test]
    fn mcp_variant_names_the_daytrace_mcp_tools() {
        let now = Local.with_ymd_and_hms(2026, 10, 17, 9, 0, 0).unwrap();
        let p = build(now, "2026-10-17", "", "", &[], &[], ToolSet::Mcp);
        assert!(p.contains("full names start with `mcp__daytrace__`"));
        assert!(p.contains("- \"what did I do\" → get_timeline for the relevant day"));
        assert!(p.contains("- \"show me\" or \"what was on screen\" → get_observations"));
        assert!(p.contains("has a `record_id`."));
        assert!(!p.contains("fetchTimeline") && !p.contains("timeByCategory"));
        // Everything else is the same prompt.
        let api = build(now, "2026-10-17", "", "", &[], &[], ToolSet::Api);
        let tail = |t: &str| t[t.find("## RESPONSE STYLE").unwrap()..t.find("## CITATIONS").unwrap()].to_string();
        assert_eq!(tail(&p), tail(&api));
        let charts = |t: &str| t[t.find("## INLINE CHARTS").unwrap()..].to_string();
        assert_eq!(charts(&p), charts(&api));
    }
}
