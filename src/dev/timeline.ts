/*
 * DEV ONLY: Timeline fixtures and handlers for the browser preview (src/dev/mock-ipc.ts merges
 * them over its own). Synthetic data only.
 *
 *   - three days ago gets a run of three failed cards (collapsed into one row with Try again)
 *   - card edits, summary feedback and Focused/Neutral/Distracted ratings are remembered
 *   - Try again and Reprocess day run through queued/running/done over a few seconds
 *   - reveal_screenshot answers like the backend (the viewer's "Show in File Explorer")
 *   - ?tl=dayerror makes get_day fail (the Timeline's error state)
 *   - ?sessions=1 (or the switch in Settings > AI tools) shows made-up Claude Code session cards on
 *     today: two on Work cards, one that no card covers, and two counted as scratch runs
 */
import type { Card, Settings } from "@/api";
import type { CardReview, DayExtras, FailedGroup, Feedback, RetryItem, SessionCard } from "@/timeline-extras-api";
import { at, shiftDay } from "@/lib/time";
import { fmtClock, type Dataset } from "@/dev/fixtures";

type Args = Record<string, unknown>;

const FAILED_REASON = "The model provider is rate limiting requests. Your recording is saved.";
const FAILED_DETAIL =
  '429 Too Many Requests: {"type":"error","error":{"type":"rate_limit_error","message":"Number of request tokens has exceeded your per-minute rate limit"}}';

function failedCard(id: number, batchId: number, day: string, startTs: number, endTs: number): Card {
  return {
    id,
    batchId,
    start: fmtClock(startTs),
    end: fmtClock(endTs),
    startTs,
    endTs,
    day,
    title: "Processing failed",
    summary: FAILED_REASON,
    category: "System",
    subcategory: "Error",
    detailedSummary: FAILED_DETAIL,
    distractions: [],
    appSites: null,
    isIdleShortcut: false,
  };
}

/** Three failed cards from 11:00 to 11:40 on the day three days back. */
function addFailedRun(ds: Dataset) {
  const day = shiftDay(ds.today, -3);
  const cards = ds.days.get(day);
  if (!cards) return;
  const from = at(day, 11, 0);
  const to = at(day, 11, 40);
  const kept = cards.filter((c) => c.endTs <= from || c.startTs >= to);
  const run = [
    failedCard(9301, 9901, day, from, at(day, 11, 15)),
    failedCard(9302, 9902, day, at(day, 11, 15), at(day, 11, 30)),
    failedCard(9303, 9903, day, at(day, 11, 30), to),
  ];
  ds.days.set(day, [...kept, ...run].sort((a, b) => a.startTs - b.startTs));
}

/** Consecutive failed cards (≤ 60 s apart) as one group (timeline_extras/cards.rs group_failed). */
function groupFailed(cards: Card[]): FailedGroup[] {
  const out: FailedGroup[] = [];
  for (const c of [...cards].sort((a, b) => a.startTs - b.startTs)) {
    if (c.category !== "System") continue;
    const last = out[out.length - 1];
    if (last && c.startTs - last.endTs <= 60) {
      last.cardIds.push(c.id);
      if (c.batchId !== null) last.batchIds.push(c.batchId);
      last.endTs = Math.max(last.endTs, c.endTs);
    } else {
      out.push({ cardIds: [c.id], batchIds: c.batchId !== null ? [c.batchId] : [], startTs: c.startTs, endTs: c.endTs, retryable: true });
    }
  }
  return out;
}

/** Made-up session cards for today: the plugin's fields, joined to the cards by finish time. */
function sessionCards(day: string, cards: Card[], now: number): SessionCard[] {
  const work = cards.filter((c) => c.category === "Work" && c.endTs - c.startTs >= 10 * 60);
  const base = { startedBy: "composer", gradedBy: "", approximate: false };
  const out: SessionCard[] = [];
  if (work[0]) {
    const at = work[0].startTs + Math.floor((work[0].endTs - work[0].startTs) * 0.8);
    out.push({
      ...base,
      cardId: work[0].id,
      sessionId: "3f6c2a1e-0b7d-4c55-9e21-7a1d5b0c9f10",
      cwd: "C:\\dev\\sync-engine",
      outcome: "The conflict resolver keeps the newer edit when two devices change the same note.",
      check: "The resolver's table-driven tests, with the equal-timestamp case added, on the real sync fixture.",
      grade: "pass",
      gradeNote: "42 tests ran and passed; the equal-timestamp test fails on the old code.",
      conditions: [
        { name: "Newer edit wins", kind: "must", state: "met" },
        { name: "Equal timestamps give one winner", kind: "must", state: "met" },
        { name: "A note is lost", kind: "fails", state: "met" },
      ],
      steps: ["Read the resolver", "Write the failing test", "Fix the ordering", "Run the fixture"],
      startedAt: at - 34 * 60,
      finishedAt: at,
    });
  }
  if (work[1]) {
    const at = work[1].startTs + Math.floor((work[1].endTs - work[1].startTs) * 0.5);
    out.push({
      ...base,
      cardId: work[1].id,
      sessionId: "b81d94c7-52aa-4e0f-8a3b-0c6e2f4d7a55",
      cwd: "C:\\dev\\site",
      outcome: "The pricing page loads the plans from the API and shows the yearly price.",
      check: "The page in a browser at phone and desktop width, against the staging API.",
      grade: "partial",
      gradeNote: "Desktop width checked in the browser. The phone width was not checked: the staging API was down.",
      gradedBy: "a fresh reviewer",
      conditions: [
        { name: "Plans come from the API", kind: "must", state: "met" },
        { name: "Phone width reads well", kind: "must", state: "open" },
      ],
      steps: ["Wire the API", "Lay out the plans", "Check both widths"],
      startedAt: at - 21 * 60,
      finishedAt: at,
    });
  }
  const last = cards.reduce((a, c) => Math.max(a, c.endTs), 0);
  // The day ends at 4 AM tomorrow; the card must also lie before "now".
  const free = Math.min(now, at(shiftDay(day, 1), 4, 0) - 60);
  if (last > 0 && free > last + 120) {
    out.push({
      ...base,
      cardId: null,
      sessionId: "0d2e7b93-6f14-4a8c-b5d0-91c3e8a7f246",
      cwd: "C:\\dev\\notes",
      outcome: "The weekly notes export writes one Markdown file per day.",
      check: "The export on last week's notes.",
      grade: "fail",
      gradeNote: "The export stopped on a note with an empty title. Not fixed in this session.",
      conditions: [{ name: "Seven files written", kind: "must", state: "failed" }],
      steps: ["Write the exporter", "Run it on last week"],
      startedAt: last + 30,
      finishedAt: last + 90,
    });
  }
  return out.sort((a, b) => a.finishedAt - b.finishedAt);
}

export function timelineMock(ds: Dataset, now: () => number, settings?: Settings): Record<string, (a: Args) => unknown> {
  addFailedRun(ds);
  const cardsOf = (day: string) => ds.days.get(day) ?? [];
  const all = () => [...ds.days.values()].flat();
  const edited = new Set<number>();
  const feedback = new Map<number, Feedback>();
  const reviews = new Map<number, CardReview>();

  // Seed: a few rated cards today, most of yesterday rated (a mix), none on other days, and one
  // summary marked as not accurate. Ratings totals come from these, per day.
  const today = cardsOf(ds.today).filter((c) => c.category === "Work");
  today.slice(0, 2).forEach((c) => reviews.set(c.id, { cardId: c.id, rating: "focused", coverage: 1 }));
  const ratingOf = (cat: string): CardReview["rating"] => (cat === "Work" ? "focused" : cat === "Distraction" ? "distracted" : "neutral");
  cardsOf(shiftDay(ds.today, -1))
    .filter((c) => c.category !== "System" && !c.isIdleShortcut)
    .slice(0, -1)
    .forEach((c) => reviews.set(c.id, { cardId: c.id, rating: ratingOf(c.category), coverage: 1 }));
  if (today[3]) feedback.set(today[3].id, { cardId: today[3].id, direction: "down", note: "This was a review, not debugging.", updatedAt: now() - 3600 });

  const extras = (day: string): DayExtras => {
    const cards = cardsOf(day);
    const rated = cards.filter((c) => reviews.has(c.id));
    const secsOf = (r: string) => rated.filter((c) => reviews.get(c.id)!.rating === r).reduce((a, c) => a + c.endTs - c.startTs, 0);
    const unreviewed = cards.filter((c) => c.category !== "System" && !reviews.has(c.id));
    return {
      day,
      edited: cards.filter((c) => edited.has(c.id)).map((c) => c.id),
      feedback: cards.filter((c) => feedback.has(c.id)).map((c) => feedback.get(c.id)!),
      reviews: rated.map((c) => reviews.get(c.id)!),
      reviewSummary: {
        focusedSecs: secsOf("focused"),
        neutralSecs: secsOf("neutral"),
        distractedSecs: secsOf("distracted"),
        lastReviewedAt: rated.length ? now() - 50 * 60 : null,
        unreviewedCards: unreviewed.length,
        firstUnreviewed: unreviewed[0]?.id ?? null,
      },
      failedGroups: groupFailed(cards),
      ...(settings?.showSessionCards && day === ds.today
        ? { sessionCards: sessionCards(day, cards, now()), sessionCardsHidden: 2 }
        : { sessionCards: [], sessionCardsHidden: 0 }),
    };
  };

  // Try again: each batch waits 2 s, runs 2 s, then its card turns into a written card.
  let retryRun: { batchIds: number[]; startedAt: number } | null = null;
  const retryItems = (): RetryItem[] => {
    if (!retryRun) return [];
    const elapsed = (Date.now() - retryRun.startedAt) / 1000;
    const total = retryRun.batchIds.length;
    const items = retryRun.batchIds.map((batchId, i): RetryItem => {
      const start = 2 + i * 2;
      const state = elapsed < start ? "queued" : elapsed < start + 2 ? "running" : "done";
      return { batchId, state, position: i + 1, total };
    });
    if (items.every((i) => i.state === "done")) {
      for (const b of retryRun.batchIds) {
        const c = all().find((x) => x.batchId === b && x.category === "System");
        if (c) {
          Object.assign(c, {
            category: "Work",
            subcategory: "Coding",
            title: "Writing tests for the sync conflict resolver",
            summary: "Wrote table-driven tests for the conflict resolver and fixed the ordering of equal timestamps.",
            detailedSummary: `${c.start} - ${c.end}: Wrote tests for the conflict resolver`,
            appSites: { primary: "Visual Studio Code", secondary: null },
          });
        }
      }
      retryRun = null;
    }
    return items;
  };

  // Reprocess day: every batch takes 1.5 s.
  let reprocess: { ids: number[]; startedAt: number } | null = null;
  const progress = (ids: number[]) => {
    if (!reprocess || ids.length === 0) return { waiting: 0, running: 0, failed: 0, step: null };
    const done = Math.floor((Date.now() - reprocess.startedAt) / 1500);
    const left = Math.max(0, ids.length - done);
    if (left === 0) reprocess = null;
    return { waiting: Math.max(0, left - 1), running: left > 0 ? 1 : 0, failed: 0, step: left > 0 ? "Reading screenshots" : null };
  };

  // ?tl=dayerror: get_day fails, to see the Timeline's error state.
  const dayError = new URLSearchParams(location.search).get("tl") === "dayerror";

  return {
    ...(dayError
      ? {
          get_day: () => {
            throw "database disk image is malformed (SQLITE_CORRUPT)";
          },
        }
      : {}),
    te_day_extras: (a) => extras(a.day as string),
    te_update_card: (a) => {
      const c = all().find((x) => x.id === a.cardId);
      if (!c) throw "card not found";
      if (typeof a.title === "string" && a.title) c.title = a.title;
      if (typeof a.category === "string" && a.category) c.category = a.category;
      if (typeof a.subcategory === "string") c.subcategory = a.subcategory;
      edited.add(c.id);
      return structuredClone(c);
    },
    te_set_feedback: (a) => {
      const id = a.cardId as number;
      if (!a.direction) {
        feedback.delete(id);
        return null;
      }
      const f: Feedback = { cardId: id, direction: a.direction as "up" | "down", note: (a.note as string) ?? "", updatedAt: now() };
      feedback.set(id, f);
      return f;
    },
    te_rate_card: (a) => {
      const id = a.cardId as number;
      if (a.rating) reviews.set(id, { cardId: id, rating: a.rating as CardReview["rating"], coverage: 1 });
      else reviews.delete(id);
      return null;
    },
    te_retry_batches: (a) => {
      retryRun = { batchIds: a.batchIds as number[], startedAt: Date.now() };
      return (a.batchIds as number[]).length;
    },
    te_retry_status: () => retryItems(),
    te_reprocess_day_plan: (a) => {
      const day = a.day as string;
      const cards = cardsOf(day);
      return { day, batchIds: cards.map((c) => c.batchId ?? 0), keptEditedCards: cards.filter((c) => edited.has(c.id)).length, blocked: null };
    },
    te_reprocess_day: (a) => {
      const day = a.day as string;
      const ids = cardsOf(day).map((c) => c.batchId ?? 0).slice(0, 6);
      reprocess = { ids, startedAt: Date.now() };
      return { day, batchIds: ids, keptEditedCards: 0, blocked: null };
    },
    te_batch_progress: (a) => progress(a.batchIds as number[]),
    reveal_screenshot: (a) => {
      const id = a.id as number;
      if (id % 97 === 13) throw "This screenshot was deleted (retention period or storage limit).";
      console.info(`[preview] reveal_screenshot(${id}): File Explorer would open with the file selected`);
      return null;
    },
  };
}
