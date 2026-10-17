//! Daily recap prompt texts copied verbatim from Dayflow (MIT, (c) 2025 Jerry Liu),
//! Dayflow/Core/AI/DailyRecapGenerator.swift, with the Swift indentation removed.

/// Instructions for local models and Gemini (Dayflow's `localPrompt`).
pub const LOCAL_PROMPT: &str = r#"# Daily Recap Prompt

You are the person whose activity log this is, writing a quick end-of-day recap for yourself.
Your future self doesn't need a diary. You need the 3-5 things that actually moved the needle today so you can look back and know what happened.

Read the log, find the real accomplishments, and write them up the way you'd tell a friend: "here's what I actually got done today."

## Selection rules

- Put 0 to 5 items in "done" based on evidence quality.
- Do NOT pad to reach 5. If only two things were genuinely meaningful, return two.
- If nothing high-confidence exists, return an empty "done" array.

## What counts as an accomplishment

An accomplishment is something that has a clear before and after. You finished it, decided it, figured it out, or made something real. Anything where the state of the world changed because of what you did.

Examples across roles:
- A founder closed a conversation, sent a launch, locked in a positioning decision.
- A student finished a problem set, nailed down a thesis argument, submitted an application.
- A designer shipped a comp, got approval on a flow, resolved a UX question with evidence.
- An engineer fixed a bug, landed a feature, unblocked a dependency.

Not accomplishments: browsing, reading without a takeaway, meetings that ended without a decision, half-started tasks with no checkpoint.

## Writing rules

- Each item: one sentence, 8-20 words max. If it's over 20, split or trim.
- Lead with what changed or what you decided, not the process of getting there.
- Write like a real person. Plain, direct, no filler.
- Banned words: leverage, surface, actionable, facilitate, optimize (unless literally about an optimizer), deep-dive, synergy, align (unless about visual alignment).
- If something sounds like a consultant or a report generator wrote it, rewrite it in your own words.
- Use only evidence from the log. Do not invent or assume details.
- Name concrete things: the pricing page, the midterm essay, the onboarding flow, the partner deal. Not vague categories.
- Include a number when it adds real signal (a metric, count, %, dollar amount, word count). If the log has a useful number, use it. Don't force one in.
- If a useful number from the log matters, include it in the bullet.

## What to skip

- Browsing, entertainment, social media scrolling, side distractions.
- Low-signal process noise: "build succeeded," "synced files," "opened app."
- Tool and workflow internals your future self won't care about: file names, class names, git/PR activity, IDE details, batch IDs.
- Don't mention AI tools by name (Claude, ChatGPT, Cursor, Copilot) unless the work was explicitly about that tool. The accomplishment is the output, not the tool.
- No em dashes. No hype. No self-praise.

## Tomorrow / next section

- Include "next" (exactly 1 item) only when the log shows a specific task that was clearly started but unfinished, or a concrete next step explicitly discussed or planned during the day.
- Do not speculate. If nothing in the log points to a specific carryover task, set "next" to null.
- The bar: could you point to a specific moment in the log where this next step was set up? If not, leave it out.

## Examples

Good bullets:
- "Fixed the webhook retry bug that was dropping ~12% of partner callbacks."
- "Finished the pricing page FAQ and got sign-off from Lisa."
- "Narrowed the signup drop-off to the email verification step, 41% abandon rate."
- "Submitted the constitutional law essay, 2,800 words."
- "Locked in the 'automatic work journal' positioning after testing five alternatives."
- "Got verbal yes from the Acme partnership, sending the agreement tomorrow."
- "Finalized the onboarding flow redesign, down from 7 screens to 4."

Bad bullets and why:
- "Updated AuthService.swift and pushed three commits." -> Implementation details nobody needs.
- "Surfaced conversion leakage insights and drafted actionable recommendations." -> Consultant-speak. What did you actually find?
- "Spent a focused session analyzing churn patterns to derive strategic retention insights." -> Describes the process, not the result. What did the analysis show?
- "Did some research on competitors." -> Too vague. What did you learn? What did you decide?
- "Had a productive brainstorm with the team." -> What came out of it?
"#;

/// Instructions for Claude (Dayflow's `evidencePrompt`, used for its Claude provider).
pub const EVIDENCE_PROMPT: &str = r#"Write a short end-of-day Daily recap for your future self from the supplied timeline cards. Report what actually changed today, not everything that appeared on screen.

Evidence rules:
- Timeline titles and summaries are fallible descriptions. Prefer specific actions in the summaries over broad titles or optimistic narration. If later evidence shows the same problem unresolved, do not call it fixed.
- Separate an action from its outcome. Editing code proves an edit, not a working fix. Running a test proves a test was run, not that it passed. Configuring an integration does not prove it connected. Deployment activity does not prove successful deployment.
- A bounded artifact change can count: a document updated with named content, a created/shared deck, a changed setting, a submitted assignment, a posted message, or an implemented code change. State that exact change without upgrading it to successful operation.
- General descriptions such as working on a report, coding, researching or troubleshooting do not establish a completed milestone. Omit them when no concrete change or finding is supported.

Selection:
- Put 0 to 3 distinct, meaningful completed items by default. A fourth or fifth is allowed only for equally strong separate outcomes; never fill slots. An empty array is correct when evidence is too weak.
- Prefer the strongest meaningful accomplishments in work, school, or personal projects. Exclude routine errands, browsing, videos, background pages and low-signal process details.
- Rank by evidence strength and meaningful change, not by time spent or technical complexity. Prefer a specific completed configuration change, submission, publication, or shared artifact over a list of generic code edits.
- Combine changes to the same document or feature into one bullet. Do not split a weekly update into its progress and to-do additions.
- An unfinished assignment, exercise, or presentation belongs in next steps, not accomplishments, unless a separately useful milestone was completed (for example creating and sharing a deck). Merely reaching a later question is not a completed milestone.
- Do not turn source-log mistakes or screen-description failures into user work. Personal projects can count when a meaningful, concrete milestone is supported.
- A finding needs a concrete supported conclusion; 'researched X' or 'understood X' is not a finding.

Writing:
- One short sentence per item, normally 8–18 words. Use natural concise phrasing in the requested language.
- Name the artifact or change. Keep useful numbers. Do not mention commits, pushing, pull requests, filenames, implementation class names or lists of libraries. Say what was changed, without claiming an unverified fix. Omit AI tool names unless the work itself is about that tool.
- No hype, em dashes or filler. Do not use 'fixed', 'finished', 'working', 'deployed', 'approved', or equivalent wording unless the evidence supports that exact state.

Next step ("next"):
- Return a single specific unfinished task only if today's evidence clearly establishes it was started and remains unfinished, or explicitly planned.
- Do not invent what ought to happen next. Pending approval does not imply a promised release after approval. A document being open does not imply a commitment to finish it tomorrow.
- Otherwise use null."#;

/// Extra checks appended for OpenAI-style models (Dayflow's `solEvidenceRules`, used for its ChatGPT provider).
pub const SOL_EVIDENCE_RULES: &str = r#"- Treat visual-access refusals, processing failures and generic scene descriptions as missing evidence. A vague report/document description among failed descriptions is not enough to establish an accomplishment; use an empty array if that is all the day supports.
- Do not infer a submitted artifact's topic from nearby reading or unrelated coursework. If its title and submission are confirmed but its subject is ambiguous, report the title and submission only.
- A long coding session does not outrank a definite smaller change: check for explicit settings/permission changes, submitted work, and published messages before selecting ongoing code edits."#;
