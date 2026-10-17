# Daytrace design contract

For everyone working on the views (Timeline, Daily, Weekly, Goals, Chat, Settings, Onboarding),
which all run on this design now. Base: designer-shadcn's proposal (shadcn/ui on Radix,
Tailwind 4, lucide), with the bespoke category palette and fluent's status block, title bar and
Contrast-theme rules.

## Files: what you own, what you leave alone

| Path | Owner | Notes |
|---|---|---|
| `src/views/<area>/**` | you | `index.tsx` default-exports the section; put everything else of your view next to it |
| `src/views/*-api.ts`, `src/timeline-extras-api.ts` | shared | typed IPC wrappers; change with the backend command |
| `src/app/*` | foundation | shell, rail, status block, banners, command menu, app state, day state, settings write queue |
| `src/components/ui/*` | foundation | shadcn copies, already edited for Daytrace; do not restyle them per view |
| `src/components/{page-header,day-nav,day-picker,banner}.tsx` | foundation | shared page parts |
| `src/design/tokens.css`, `src/index.css` | foundation | no new colors, sizes or global rules from views |
| `src/lib/{router,time,theme,categories,utils}.ts` | foundation | add pure helpers only after asking |
| `src/dev/*` | everyone, carefully | fixtures may grow; keep them synthetic (invented people, invented days) |
| `src-tauri/**` | backend | not part of the view work |

Need a token, a component variant or a shell change? Write it in your notes file
(`scratchpad/agents/<you>.md`, section "Asks for foundation") instead of editing those files.

## Run it

- `npm run dev`, open `http://localhost:1420/` in a browser. Outside Tauri the IPC is mocked with
  synthetic data (`src/dev/mock-ipc.ts`). Variants go before the hash and survive navigation:
  `?status=recording|paused|paused-timed|locked|away|capture-error|starting`,
  `?status=writing|batch|queued|offline|failing|nokey` (comma-separated to combine),
  `?onboarding=1`, `?empty=1`, `?appearance=light|dark`, `?provider=ollama`, `?latency=400`,
  `?chat=empty`, `?chat=charts` (chats with inline charts: `#/chat?c=91`, and blocks that can't be
  drawn: `#/chat?c=92`), `?fail=apikey|credits|limit|busy|flaky|login|cli|slow|model|unknown` (the last
  batch failed for that cause), `?badge=recap` (a new recap: dot on Daily), `?gen=off` (cards
  without `generatedBy`). Example: `/?status=offline,away#/daily?day=2026-10-16`.
- Fixture days are relative to today: today a workday (with a failed card, an idle card, a
  40-minute gap, interruptions), yesterday a night-owl day (9 PM to 2:40 AM), two days ago empty,
  then generated days. If a view needs more (a recap with blockers, a goal day), add it there.
- Screenshots: `npm run screenshots -- --out <folder>` shoots the default routes (every section and
  the main states, named `<section>-<state>`) at 1280×800 in light and dark, clock fixed at Fri 16
  Oct 2026, 5:42 PM. `--only daily`, `--widths 1280,760`, `--forced` (Windows Contrast theme),
  `--route name='/?empty=1#/daily|w=760|keys=Control+k'`, `--url http://127.0.0.1:1420` to reuse a
  running server. Files: `<name>-<theme>[-<width>][-forced].png`. Look at your screenshots in both
  themes and at 760 px before you hand over.
- `npm run contrast` prints the measured token pairs. `npx tsc --noEmit && npm run build` must
  pass; the build also runs `scripts/check-dist.mjs` (CSP and dev-mock checks).

## Color

Color is data: chrome is graphite and ink; hue belongs to categories and status. Use the
Tailwind names, never hex or `rgb()` in views.

| Role | Classes | Use |
|---|---|---|
| canvas | `bg-background` | the rail and window; not inside the content layer |
| content layer | `bg-surface-1` | the section panel (already set by the shell), cards, panels |
| wells, hover, selected | `bg-surface-2` | hover rows, selected list item, inset wells |
| pressed, tracks | `bg-surface-3` | strip tracks, pressed state |
| text | `text-foreground` / `text-muted-foreground` / `text-subtle-foreground` | body / secondary / tertiary (all ≥ 4.5:1 on surface-1, background, surface-2) |
| lines | `border` (= `border-border`) / `border-border-strong` | decorative hairlines; never the only cue |
| control outline | `border-input` | inputs, checkboxes (≥ 3:1) |
| primary action | `bg-primary text-primary-foreground` | ink button, one per area |
| failure | `text-destructive`, `bg-destructive-surface` | errors, failed cards, destructive buttons |
| status | `bg-live`, `text-live-foreground`, `bg-paused`, `text-warn-foreground`, `bg-warn-surface` | recording / paused / warnings, always with words |
| categories | `colorFor(name)` (see below), `--cat-work` … | data marks only |
| charts | `--chart-*`, `--chart-axis`, `--chart-grid`, `--heat-0…4` | hand-drawn SVG; heat ramp is one hue (plum) |
| timeline | `--trace-track`, `--trace-hatch`, `--trace-now`, `--trace-gridline` | day strip and rails |

**Category colors.** Take `colorFor` from the section props or `useAppState()`. It returns a
CSS color for a card's category name: the theme-aware token for the recommended defaults, the
user's hex otherwise, `--cat-system` for failed cards, a neutral for unknown names. Do not color
by the `color` field of insights payloads (it is the stored hex, wrong in dark mode). Rules:

- Every colored mark has its category name next to it (label, legend, row text or tooltip plus a
  table). Personal and the recording green share a hue; the words keep them apart.
- Idle is texture: `.hatch-idle` (stroke `--cat-idle`), never a solid block. Not recorded:
  `.hatch-gap`. Failed / not analyzed: `.stripes-failed` plus the words "Not analyzed".
- Give marks `data-mark` (or an inline `background`) so Windows Contrast themes keep them visible.
- The idle hatch is under 3:1 on `surface-3` tracks in light mode (2.7:1); draw it over
  `surface-1` or the canvas where it is the only cue.

## Type, space, shape

| Class | Size / line | Use |
|---|---|---|
| `text-axis` + `font-mono` | 11/16 | hour labels on axes only |
| `text-caption` | 12/16 | durations, notes, legends |
| `text-meta` | 13/18 | secondary lines, table cells |
| `text-body` | 14/20 | default; titles at `font-semibold` |
| `text-lead` | 15/24 | card summary, recap text |
| `text-title` + `font-display font-semibold` | 20/28 | page and card titles (h1 is set by PageHeader) |
| `text-display` + `font-display` | 28/36 | stat values |

- Clock times in `font-mono` (Cascadia Mono, bundled as Daytrace Mono); numbers that line up get
  `tnum`. Durations from `formatDuration` (no-break spaces: `1 h 30 min`).
- 4 px grid. Page padding `px-(--page-x)` (20 px), section gaps `gap-6`, card padding `p-4`, list
  rows `py-2 px-3`. Controls 32 px (`h-8`, Button default), small 28 px; nothing under 24 px.
- Radii: controls `rounded-sm` 4 px, cards `rounded-md` 6 px, panels `rounded-lg` 8 px, dialogs
  `rounded-xl`. Shadows: `shadow-xs` lift, `shadow-md` selected card and popovers, `shadow-lg`
  dialogs and toasts.
- Breakpoints (window width): `split:` 960 px (list and detail side by side), `wide:` 1100 px
  (rail labels), `xwide:` 1800 px (ultrawide). The window is never narrower than 760 px; at 760
  the rail is 56 px and the content layer about 690 px wide.
- Density: one idea per row, 8–12 px between rows, no card-in-card-in-card. Desktop app, not a
  web page: no hero blocks, no marketing copy.

## Components (`src/components/ui`)

Button (`default` ink, `outline`, `secondary`, `ghost`, `destructive`, `link`; sizes `sm`
`default` `lg`, `icon-sm` `icon` `icon-lg`), Input, Textarea, NativeSelect (use for every single
choice: Windows draws the list; `wrapperClassName="w-full"` to fill a column), Checkbox,
RadioGroup, Switch, Label, Toggle and ToggleGroup (pressed = ink fill, Highlight in Contrast
themes; do not restyle the pressed state per view), DropdownMenu (actions: "More", "Export…"),
Popover (pickers), Tooltip (names of icon-only buttons; never essential text), Dialog,
AlertDialog (destructive confirmations whose buttons say what happens: "Delete 3.4 GB of
screenshots"), Command (cmdk), Kbd, Skeleton, Spinner, Empty, Toast. Shared: `PageHeader`,
`TitleBadge`, `DayNav`, `DayPicker`, `Banner` (`details` puts the raw error in a collapsed
"Error details"). Unused shadcn parts (Tabs, Slider, Separator) were removed; ask to bring one back.
No other UI libraries; ask before adding one (bundle and CSP). Charts are hand-written SVG.

## Page pattern

```tsx
export default function DailySection() {
  const { day, today, colorFor, openTimeline } = useViewProps();
  return (
    <>
      <PageHeader label="Daily" title={formatDayTitle(day)} nav={<DayNav />}
        badge={day === today ? <TitleBadge>Today</TitleBadge> : undefined}
        actions={<Button variant="outline" size="sm">Copy as text</Button>} />
      <div className="scroll-thin min-h-0 flex-1 overflow-y-auto px-(--page-x) py-5">…</div>
    </>
  );
}
```

- The shell gives you a column (`flex flex-col`, overflow hidden). PageHeader first (it renders
  the one `h1` and the global banners under it), then your own scroll region.
- Day sections use `DayNav` (Weekly: `<DayNav unit="week" />`, title `formatWeekRange(weekStart(day))`).
- Headings inside the page start at `h2` (`text-body font-semibold` for section titles).

## State, routes, keys

- `useAppState()`: settings, status (polled every 5 s by the shell), `colorFor`, `recorder` and
  `analysis` wording, `problems`, `reload()`, `setSettings(dto)`. Never poll `get_status` yourself.
  It re-renders on a status change only when you read a status field (`status`, `today`,
  `recorder`, `analysis`, `problems`) while rendering. Narrower: `useAppSettings()` (settings,
  categories, colors, actions; never re-renders on status), `useAppStatusSelect(s => …)` (one
  value from the status; re-renders when that value changes).
- `useDay()` / section props: `day`, `today`, `setDay(d)`, `openTimeline(day, cardId?)`.
- Routes: `#/<section>?day=…&…`. `useRoute()`, `navigate(section, params)`, `setParams(patch)`
  (replaces history: use it for selection, tabs inside a page, open panels; from an effect pass
  `{ section: "<yours>" }` so a late effect cannot write into the next section's URL). Put every
  state worth linking to in the URL (`card`, `tab`, `section`, `view=table`); read it back on load.
- Shell keys: Ctrl+K, Ctrl+1…5, Ctrl+,, `[` and `]` (DayNav). Timeline adds J/K. Check
  `isTypingTarget(e.target)` (`lib/utils`) before handling a plain key; do not bind Ctrl+F/P/R.
- Match shortcuts by physical key, `e.code`, with `lib/keys.ts` (`isKey(e, "KeyJ")`, `digitOf`):
  on a Belgian AZERTY keyboard Ctrl+1 gives `e.key` "&" and `[` needs AltGr. Show the key with
  `useKeyLabel(code)` (the layout's own character, "^" for BracketLeft on AZERTY). Arrows, Enter,
  Escape, Space, Home/End can stay on `e.key`.
- Dialogs (`Dialog`, `AlertDialog`) give focus back to the element that opened them, or to the
  menu trigger when a menu item opened them. When that element is gone (the dialog opened from a
  link, or it deleted the thing it was about), pass `returnFocus={() => element}` to the
  content. A control that replaces itself (Pause → Resume, Edit → Save) moves the focus to its
  replacement.
- DayNav's Today / This week button sits right of the title (PageHeader), so the title never
  moves; keep the `badge` for today only.

## States

- Loading: show nothing for 150 ms, then a `Skeleton` in the shape of the content; keep it at
  least 300 ms (`useDelayedLoading` in `lib/use-delayed.ts`). Polling refreshes must not show
  loaders or move anything (compare, then set).
- Empty: `Empty` with what happened and the next step ("Nothing recorded on Sunday 11 October" + Go
  to today). Errors that belong to the view: inline `Banner tone="error"` with the fix as a button
  and the raw message in `details` (a collapsed "Error details"). Toasts only confirm finished actions
  ("Exported", "Copied") and offer Undo; never for errors that need action.
- Global problems (capture failing, no key, provider unreachable, and every other reason the
  last batch failed: "Timeline cards can’t be written" with the cause, its fix and the raw error,
  `app/failure.ts`) are already shown under every PageHeader and in the rail. Do not repeat them
  inside views.

## Focus and accessibility

- Every focusable element shows the global 2 px `--ring` outline with a 2 px offset on keyboard
  focus. Do not add `outline-none` without a visible replacement; inside scroll areas use
  `focus-visible:-outline-offset-2` so the outline is not clipped.
- Composite widgets (strips, grids, lists of cards) are one tab stop with arrow keys inside
  (roving `tabIndex`). Icon-only buttons need `aria-label` and a Tooltip. Status text that
  changes by itself goes in `role="status"` or `aria-live="polite"`, never the whole region.
- Text pairs ≥ 4.5:1, marks and control outlines ≥ 3:1 (`npm run contrast`). Animate only
  `opacity` and `transform`, with `--dur-fast/base/slow`; all of it stops under reduced motion.
- Mark app and site names `translate="no"`. Sentence case, plain words, `…` for loading text and
  follow-up actions, real numbers.

## CSP (Tauri: `style-src 'self' 'unsafe-inline'`)

- No inline `<style>` in `index.html` (the build fails) and no library that injects `<style>`
  at runtime (sonner, CSS-in-JS). React `style={{…}}` props are fine.
- No network: fonts, images and scripts come from the bundle. Screenshots come from IPC as data
  URLs (`img-src 'self' data:`).

## Settings writes

Every settings change goes through `savePatch()` in `app/settings-writes.ts` (Settings pages via
`useSave()`, the rail's theme switch, the command menu): one queue, each write a patch on a fresh
`get_settings`. Put the returned settings into the app state with `setSettings(dto)`; the app
state applies reads and writes in order, so a refetch that started before a save cannot show the
old value again.

## Words (glossary)

US English everywhere, aria-labels and code comments included: color, colored, license, analyze,
canceled, behavior, gray. Plain literal words, sentence case. Use these terms and no variants:

| Thing | Say | Not |
|---|---|---|
| Recorder state | "Recording", "Paused", "Not recording" (recorder off) | "Capturing", "Stopped" |
| Time without screenshots | "Not recorded" | "No recording", "Not recording" |
| Time with screenshots | "Recorded" | "Time captured", "Captured" |
| Recorded minus idle | "Active time" | "Active" alone |
| Percentages of the day | "… of active time", with active time as the denominator | "of captured", "of recorded" |
| A failed card | "Not analyzed" | "Failed", "Error" |
| Retry action | "Try again" | "Retry", "Try now" (except the capture and network banners' "Try now") |
| Goal targets | "Focus target", "Distraction limit" | "Distraction budget" |
| Focus streak | "Longest focus block" | "Longest focus" |
| Switches between apps | "Context switches" | "Context shifts", "Context switched" |
| Rating prompt | "Rate the next card" | "Review the next card" |
| Zero durations | "0 min" | "–", "None" |
| Export | header button "Export…"; dialog primary "Export as Markdown", secondary "Copy day as text" | "Export Markdown…" |
