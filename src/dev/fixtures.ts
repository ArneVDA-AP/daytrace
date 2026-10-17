/*
 * DEV ONLY. Synthetic data for the browser preview: an invented person's invented days. Nothing
 * here comes from a real Daytrace database, and nothing here ships (see src/main.tsx).
 *
 * Days are relative to the current logical day (4 AM rule), so the preview always has a today:
 *   today        a workday up to now (cards end about 10 minutes before the clock)
 *   yesterday    a night-owl day: a short afternoon, then 9 PM to 2:40 AM
 *   2 days ago   no recording at all (empty day)
 *   3–13 days    generated workdays and lighter weekends (seeded, the same on every load)
 */
import type { Card, Category, Distraction } from "@/api";
import { at, dateOf, dayKey, logicalDay, shiftDay } from "@/lib/time";

export const DEFAULT_CATEGORIES: Category[] = [
  {
    name: "Work",
    color: "#407BC5",
    description: "Career, school, or productivity-focused activities (projects, emails, assignments, video calls, learning skills, etc.)",
    isIdle: false,
  },
  {
    name: "Personal",
    color: "#287C42",
    description: "Purposeful non-work activities or life tasks (paying bills, fitness tracking, meal planning, personal research, creative hobbies, etc.)",
    isIdle: false,
  },
  {
    name: "Distraction",
    color: "#73176F",
    description: "Passive consumption or aimless browsing (scrolling feeds, watching random videos, clicking through news, mindless games, etc.)",
    isIdle: false,
  },
  { name: "Idle", color: "#A0AEC0", description: "", isIdle: true },
];

/** h:mm AM, as the backend formats card times. */
export function fmtClock(ts: number): string {
  return new Date(ts * 1000).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
}

interface Tpl {
  /** "9:05" 24-hour wall clock; hours < 4 belong to the next calendar date. */
  from: string;
  to: string;
  cat: "Work" | "Personal" | "Distraction" | "Idle" | "System";
  sub: string;
  title: string;
  summary: string;
  apps?: [string, string?];
  steps?: string[];
  distractions?: { from: string; to: string; title: string; summary: string }[];
}

const hm = (s: string): [number, number] => s.split(":").map(Number) as [number, number];

const WORKDAY: Tpl[] = [
  {
    from: "8:52", to: "9:20", cat: "Work", sub: "Debugging",
    title: "Triaging overnight CI failures in the sync service",
    summary: "Went through the three failed nightly runs, found that two came from a flaky network test and one from a real regression in the conflict resolver, and opened an issue for the regression.",
    apps: ["Windows Terminal", "github.com"],
    steps: ["Opened the CI dashboard and filtered the failed nightly runs", "Re-ran the flaky network test locally; it passed three times", "Bisected the resolver regression to yesterday's merge", "Opened an issue with the failing input attached"],
  },
  {
    from: "9:20", to: "10:05", cat: "Work", sub: "Meetings",
    title: "Pairing on the offline queue retry logic",
    summary: "Video call with a teammate, sharing the editor. Agreed on exponential backoff with a cap of five minutes and wrote the first version of the retry scheduler together.",
    apps: ["Microsoft Teams", "Visual Studio Code"],
    steps: ["Joined the pairing call and shared the editor", "Sketched the backoff schedule in a comment", "Wrote the retry scheduler and two unit tests", "Agreed to finish the persistence part after lunch"],
    distractions: [{ from: "9:41", to: "9:44", title: "Checked a parcel delivery notification", summary: "Opened the delivery tracking page from a phone notification." }],
  },
  {
    from: "10:05", to: "10:20", cat: "Distraction", sub: "Video",
    title: "Watching a mechanical keyboard review",
    summary: "Watched most of a review of a low-profile mechanical keyboard and read the comments.",
    apps: ["youtube.com"],
  },
  {
    from: "10:20", to: "11:35", cat: "Work", sub: "Coding",
    title: "Writing the migration for the activity table",
    summary: "Wrote and tested the schema migration that splits the activity table by day. Fixed an off-by-one in the day boundary at 4 AM and added a test for daylight saving time.",
    apps: ["Visual Studio Code", "docs.rs"],
    steps: ["Created the migration file and the new table definition", "Copied rows in batches of 10,000 to keep the lock short", "Found an off-by-one at the 4 AM day boundary", "Added a daylight saving time test case", "Ran the full test suite: 412 passed"],
  },
  {
    from: "11:35", to: "11:50", cat: "Personal", sub: "Errands",
    title: "Booking a dentist appointment and paying an invoice",
    summary: "Booked a check-up for next Tuesday through the practice's website and paid the electricity invoice in online banking.",
    apps: ["Microsoft Edge", "bank.example"],
  },
  {
    from: "11:50", to: "12:40", cat: "Idle", sub: "Idle",
    title: "Away from the computer",
    summary: "No screen changes; the screen was idle or locked.",
  },
  {
    from: "13:20", to: "14:05", cat: "Work", sub: "Code review",
    title: "Reviewing pull requests for the export feature",
    summary: "Reviewed two pull requests for the Markdown export. Asked for a test around empty days and approved the date picker change.",
    apps: ["github.com", "Visual Studio Code"],
    steps: ["Read the export pull request and ran it locally", "Left four comments, one about empty days", "Approved the date picker pull request"],
    distractions: [{ from: "13:41", to: "13:46", title: "Scrolled the Reddit front page", summary: "Opened Reddit in a new tab and scrolled for a few minutes." }],
  },
  {
    from: "14:05", to: "14:20", cat: "System", sub: "Error",
    title: "Processing failed",
    summary: "The model provider is rate limiting requests. Your recording is saved.",
  },
  {
    from: "14:20", to: "15:30", cat: "Work", sub: "Debugging",
    title: "Profiling the slow start of the desktop app",
    summary: "Recorded a startup trace, found that most of the time went into reading the settings file twice, and moved the second read behind a cache.",
    apps: ["Visual Studio Code", "Windows Performance Analyzer"],
    steps: ["Recorded three cold-start traces", "Found two settings reads on the startup path", "Cached the parsed settings", "Startup went from 1.9 s to 0.8 s in the trace"],
  },
  {
    from: "15:30", to: "15:45", cat: "Distraction", sub: "Social media",
    title: "Browsing PC build threads on Reddit",
    summary: "Read threads about small-form-factor builds and compared two cases.",
    apps: ["reddit.com"],
  },
  {
    from: "15:45", to: "16:40", cat: "Work", sub: "Planning",
    title: "Writing the weekly status update and planning tickets",
    summary: "Wrote the weekly update for the team and split the sync conflict work into five tickets with estimates.",
    apps: ["Linear", "Notion"],
  },
  {
    from: "16:40", to: "17:05", cat: "Personal", sub: "Shopping",
    title: "Ordering groceries for the weekend",
    summary: "Filled an online grocery basket from a shopping list and picked a delivery slot for Saturday morning.",
    apps: ["Microsoft Edge", "shop.example"],
  },
  {
    from: "17:05", to: "17:30", cat: "Work", sub: "Email",
    title: "Answering support emails about sync conflicts",
    summary: "Answered four support emails about duplicated notes after a sync conflict and linked the issue from this morning.",
    apps: ["Outlook"],
  },
];

const NIGHT_OWL: Tpl[] = [
  {
    from: "13:40", to: "14:25", cat: "Personal", sub: "Admin",
    title: "Sorting out the car insurance renewal",
    summary: "Compared two renewal offers and filled in the online form for the cheaper one.",
    apps: ["Microsoft Edge", "insurer.example"],
  },
  {
    from: "14:25", to: "15:10", cat: "Distraction", sub: "Video",
    title: "Watching speedrun videos",
    summary: "Watched three speedrun videos back to back.",
    apps: ["youtube.com"],
  },
  {
    from: "21:05", to: "22:30", cat: "Work", sub: "Coding",
    title: "Building the command menu for the desktop app",
    summary: "Added a command menu with sections, recent days and actions, and wired Ctrl+K to open it.",
    apps: ["Visual Studio Code", "Windows Terminal"],
    steps: ["Added the command dialog", "Listed sections with their shortcuts", "Added the last four days", "Wired Ctrl+K and Escape"],
    distractions: [{ from: "21:52", to: "21:58", title: "Read a news article", summary: "Read an article about a space launch." }],
  },
  {
    from: "22:30", to: "22:45", cat: "Distraction", sub: "Social media",
    title: "Scrolling short videos",
    summary: "Scrolled through short videos.",
    apps: ["youtube.com"],
  },
  {
    from: "22:45", to: "0:40", cat: "Work", sub: "Coding",
    title: "Fixing keyboard navigation in the day calendar",
    summary: "Made the calendar one tab stop with arrow keys inside, fixed focus after changing months and added tests for both.",
    apps: ["Visual Studio Code", "developer.mozilla.org"],
  },
  {
    from: "0:40", to: "1:10", cat: "Idle", sub: "Idle",
    title: "Away from the computer",
    summary: "No screen changes; the screen was idle or locked.",
  },
  {
    from: "1:10", to: "2:40", cat: "Work", sub: "Writing",
    title: "Writing the design notes for the timeline",
    summary: "Wrote notes on how the timeline should show gaps, idle time and failed cards, with sketches of three layouts.",
    apps: ["Obsidian"],
  },
];

/** Small deterministic PRNG (mulberry32) so generated days are the same on every load. */
function rng(seed: number) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const GEN_WORK: Omit<Tpl, "from" | "to">[] = [
  { cat: "Work", sub: "Coding", title: "Implementing the settings import", summary: "Wrote the import of settings from a JSON file with validation and clear errors.", apps: ["Visual Studio Code", "Windows Terminal"] },
  { cat: "Work", sub: "Meetings", title: "Weekly planning call", summary: "Planning call with the team: priorities for the week and who takes which ticket.", apps: ["Microsoft Teams"] },
  { cat: "Work", sub: "Code review", title: "Reviewing the storage cleanup changes", summary: "Reviewed the pull request that deletes old screenshots by size and asked for a dry-run mode.", apps: ["github.com"] },
  { cat: "Work", sub: "Writing", title: "Writing the release notes", summary: "Collected the merged changes since the last release and wrote the notes.", apps: ["Notion", "github.com"] },
  { cat: "Work", sub: "Debugging", title: "Chasing a crash on resume from sleep", summary: "Reproduced a crash after the laptop resumed from sleep and traced it to a stale capture session.", apps: ["Visual Studio Code", "Event Viewer"] },
  { cat: "Work", sub: "Learning", title: "Reading about WebView2 performance", summary: "Read documentation and a blog post about WebView2 startup and memory.", apps: ["learn.microsoft.com"] },
  { cat: "Work", sub: "Email", title: "Clearing the inbox", summary: "Answered and archived about twenty emails.", apps: ["Outlook"] },
];
const GEN_PERSONAL: Omit<Tpl, "from" | "to">[] = [
  { cat: "Personal", sub: "Errands", title: "Paying bills and checking the budget", summary: "Paid two bills and updated the monthly budget sheet.", apps: ["Excel", "bank.example"] },
  { cat: "Personal", sub: "Hobby", title: "Planning a cycling route", summary: "Planned a 60 km route for the weekend and checked the weather.", apps: ["route-planner.example"] },
  { cat: "Personal", sub: "Learning", title: "Practising Spanish", summary: "Did a lesson and reviewed vocabulary cards.", apps: ["language-app.example"] },
];
const GEN_DISTRACTION: Omit<Tpl, "from" | "to">[] = [
  { cat: "Distraction", sub: "Video", title: "Watching videos", summary: "Watched a few videos from the recommendations.", apps: ["youtube.com"] },
  { cat: "Distraction", sub: "Social media", title: "Scrolling Reddit", summary: "Scrolled the front page and a few threads.", apps: ["reddit.com"] },
  { cat: "Distraction", sub: "News", title: "Reading the news", summary: "Read headlines and two longer articles.", apps: ["news.example"] },
];

function generated(day: string): Tpl[] {
  const r = rng(Number(day.replaceAll("-", "")));
  const dow = dateOf(day).getDay(); // 0 Sunday
  const weekend = dow === 0 || dow === 6;
  const out: Tpl[] = [];
  let t = (weekend ? 10 * 60 : 8 * 60 + 30) + Math.floor(r() * 40);
  const end = weekend ? 16 * 60 : 18 * 60;
  const pick = <T,>(xs: T[]) => xs[Math.floor(r() * xs.length)];
  const fmt = (m: number) => `${Math.floor(m / 60)}:${String(m % 60).padStart(2, "0")}`;
  while (t < end) {
    const roll = r();
    const base = weekend ? (roll < 0.5 ? pick(GEN_PERSONAL) : roll < 0.8 ? pick(GEN_DISTRACTION) : pick(GEN_WORK)) : roll < 0.68 ? pick(GEN_WORK) : roll < 0.84 ? pick(GEN_DISTRACTION) : pick(GEN_PERSONAL);
    const len = base.cat === "Distraction" ? 10 + Math.floor(r() * 25) : 25 + Math.floor(r() * 70);
    out.push({ ...base, from: fmt(t), to: fmt(Math.min(end, t + len)) });
    t += len;
    if (r() < 0.12) {
      out.push({ cat: "Idle", sub: "Idle", title: "Away from the computer", summary: "No screen changes; the screen was idle or locked.", from: fmt(t), to: fmt(t + 20 + Math.floor(r() * 40)) });
      t += 60;
    } else if (r() < 0.2) {
      t += 15 + Math.floor(r() * 45); // not recording
    }
  }
  return out;
}

let nextBatch = 700;

function build(day: string, tpls: Tpl[], dayIndex: number, nowTs: number): Card[] {
  const cards: Card[] = [];
  tpls.forEach((t, i) => {
    const [fh, fm] = hm(t.from);
    const [th, tm] = hm(t.to);
    const startTs = at(day, fh, fm);
    const endTs = at(day, th, tm);
    if (endTs > nowTs - 10 * 60) return; // not written yet
    const distractions: Distraction[] = (t.distractions ?? []).map((d) => {
      const [a, b] = hm(d.from);
      const [c, e] = hm(d.to);
      return { startTime: fmtClock(at(day, a, b)), endTime: fmtClock(at(day, c, e)), title: d.title, summary: d.summary };
    });
    const isIdle = t.cat === "Idle";
    const system = t.cat === "System";
    const steps = t.steps ?? [];
    const stepLen = steps.length ? (endTs - startTs) / steps.length : 0;
    const detailed = system
      ? "429 Too Many Requests: {\"type\":\"error\",\"error\":{\"type\":\"rate_limit_error\",\"message\":\"Number of request tokens has exceeded your per-minute rate limit\"}}"
      : steps.map((s, k) => `${fmtClock(startTs + k * stepLen)} - ${fmtClock(startTs + (k + 1) * stepLen)}: ${s}`).join("\n");
    cards.push({
      id: dayIndex * 100 + i + 1,
      batchId: nextBatch++,
      start: fmtClock(startTs),
      end: fmtClock(endTs),
      startTs,
      endTs,
      day,
      title: t.title,
      summary: t.summary,
      category: t.cat,
      subcategory: t.sub,
      detailedSummary: detailed,
      distractions,
      appSites: t.apps ? { primary: t.apps[0], secondary: t.apps[1] ?? null } : null,
      isIdleShortcut: isIdle,
    });
  });
  return cards;
}

export interface Dataset {
  today: string;
  days: Map<string, Card[]>;
}

/** All synthetic days, relative to `nowTs`. `empty` gives a fresh install without cards. */
export function makeDataset(nowTs: number, empty = false): Dataset {
  const today = logicalDay(nowTs);
  const days = new Map<string, Card[]>();
  if (empty) return { today, days };
  nextBatch = 700;
  days.set(today, build(today, WORKDAY, 0, nowTs));
  const y = shiftDay(today, -1);
  days.set(y, build(y, NIGHT_OWL, 1, nowTs));
  days.set(shiftDay(today, -2), []);
  for (let i = 3; i <= 13; i++) {
    const d = shiftDay(today, -i);
    days.set(d, build(d, generated(d), i, nowTs));
  }
  return { today, days };
}

export function allCards(ds: Dataset): Card[] {
  return [...ds.days.values()].flat();
}

export { dayKey };
