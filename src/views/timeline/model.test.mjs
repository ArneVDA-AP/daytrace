// Run: TZ=Europe/Brussels node --experimental-strip-types --test src/views/timeline/model.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { at } from "../../lib/time.ts";
import { reviewQueue } from "./model.ts";
import {
  DAY_SECS,
  columnGaps,
  columnRecorded,
  displaySegments,
  firstStop,
  moveInWeek,
  offsetTicks,
  tickHour,
  wallOffset,
  weekAxis,
  weekColumns,
  weekDays,
} from "./week-model.ts";

const H = 3600;
let nextId = 1;

/** A card on `day` from h1:m1 to h2:m2 (hours before 4 belong to the next date, like at()). */
function card(day, h1, m1, h2, m2, category = "Work", extra = {}) {
  const startTs = at(day, h1, m1);
  const endTs = at(day, h2, m2);
  return {
    id: extra.id ?? nextId++,
    batchId: null,
    start: "",
    end: "",
    startTs,
    endTs,
    day,
    title: category === "System" ? "Processing failed" : `${category} ${h1}:${m1}`,
    summary: "",
    category,
    subcategory: "",
    detailedSummary: "",
    distractions: [],
    appSites: null,
    isIdleShortcut: false,
    ...extra,
  };
}

test("the week of a day runs Monday to Sunday", () => {
  assert.deepEqual(weekDays("2026-10-16"), ["2026-10-12", "2026-10-13", "2026-10-14", "2026-10-15", "2026-10-16", "2026-10-17", "2026-10-18"]);
  assert.deepEqual(weekDays("2026-10-12"), weekDays("2026-10-18"));
});

test("wall offset counts from 4 AM, also after midnight and on a DST change day", () => {
  assert.equal(wallOffset(at("2026-10-16", 4, 0)), 0);
  assert.equal(wallOffset(at("2026-10-16", 9, 30)), 5.5 * H);
  assert.equal(wallOffset(at("2026-10-16", 2, 40)), 22 * H + 40 * 60);
  // 25 October 2026: clocks go back at 3 AM in Brussels; 9 AM stays at the 9 AM height.
  assert.equal(wallOffset(at("2026-10-25", 9, 0)), 5 * H);
});

test("a run of failed cards is one item; a shorter overlapping card keeps its time", () => {
  const d = "2026-10-13";
  const [from, to] = [at(d, 4, 0), at(d, 4, 0) + 24 * H];
  const segs = displaySegments(
    [
      card(d, 11, 0, 11, 15, "System"),
      card(d, 11, 15, 11, 30, "System"),
      card(d, 11, 31, 11, 40, "System"),
      card(d, 12, 0, 13, 0, "Work", { id: 50 }),
      card(d, 12, 20, 12, 30, "Distraction", { id: 51 }),
      card(d, 14, 0, 14, 30, "Work", { id: 52 }),
      card(d, 14, 0, 14, 10, "Personal", { id: 53 }),
    ],
    from,
    to,
  );
  const failed = segs.filter((s) => s.failed);
  assert.equal(failed.length, 1);
  assert.equal(failed[0].count, 3);
  assert.equal(failed[0].end - failed[0].start, 40 * 60);
  // The 10-minute distraction inside the hour: the hour keeps its larger side (12:30 to 1 PM).
  const work = segs.find((s) => s.card.id === 50);
  assert.deepEqual([work.start, work.end], [at(d, 12, 30), at(d, 13, 0)]);
  assert.ok(segs.some((s) => s.card.id === 51));
  // Same start: the shorter card keeps it, the longer starts after it.
  const w52 = segs.find((s) => s.card.id === 52);
  assert.deepEqual([w52.start, w52.end], [at(d, 14, 10), at(d, 14, 30)]);
  // A card covered completely by a shorter one is not drawn.
  const twice = displaySegments([card(d, 15, 0, 15, 30, "Work", { id: 60 }), card(d, 15, 0, 15, 30, "Personal", { id: 61 })], from, to);
  assert.equal(twice.length, 1);
});

test("columns place cards in seconds since 4 AM and clip them to their day", () => {
  const days = weekDays("2026-10-16");
  const cols = weekColumns(days, [
    [card(days[0], 9, 0, 10, 0)],
    [],
    null,
    // Thursday: a night-owl card that runs past 4 AM is cut at the end of the day.
    [card(days[3], 21, 0, 22, 0), card(days[3], 3, 30, 3, 30, "Work", { endTs: at(days[3], 3, 30) + H })],
    [],
    [],
    [],
  ]);
  assert.deepEqual([cols[0][0].start, cols[0][0].end, cols[0][0].col], [5 * H, 6 * H, 0]);
  assert.equal(cols[2].length, 0);
  assert.deepEqual([cols[3][1].start, cols[3][1].end], [23.5 * H, DAY_SECS]);
  assert.equal(columnRecorded(cols[3]), 1.5 * H);
});

test("the week axis fits the recorded hours of all days together", () => {
  const days = weekDays("2026-10-16");
  const cols = weekColumns(days, [[card(days[0], 9, 10, 9, 50)], [card(days[1], 10, 0, 11, 20)], [card(days[2], 21, 0, 21, 30)], [], [], [], []]);
  const fit = weekAxis(cols, null, true);
  // 9 to 11:20 → 9 AM to 12 PM; 9 PM card → 9 to 10 PM; the 9-hour stretch between is cut.
  assert.deepEqual(fit.windows, [
    { start: 5 * H, end: 8 * H },
    { start: 17 * H, end: 18 * H },
  ]);
  assert.equal(fit.shown, 4 * H);
  // Now (5:42 PM on today) keeps its hour on the axis; 8 PM (within an hour of the 9 PM card) joins it.
  assert.deepEqual(weekAxis(cols, 13 * H + 42 * 60, true).windows, [
    { start: 5 * H, end: 8 * H },
    { start: 13 * H, end: 14 * H },
    { start: 17 * H, end: 18 * H },
  ]);
  assert.deepEqual(weekAxis(cols, 16 * H + 20 * 60, true).windows, [
    { start: 5 * H, end: 8 * H },
    { start: 16 * H, end: 18 * H },
  ]);
  assert.deepEqual(weekAxis(cols, null, false).windows, [{ start: 0, end: DAY_SECS }]);
  assert.deepEqual(weekAxis(weekColumns(days, [[], [], [], [], [], [], []]), null, true).windows, [{ start: 0, end: DAY_SECS }]);
});

test("hour ticks are whole hours from 4 AM, every `step` hours", () => {
  const axis = { windows: [{ start: 5 * H, end: 9 * H }], shown: 4 * H };
  assert.deepEqual(offsetTicks(axis, 1).map(tickHour), [9, 10, 11, 12, 13]);
  assert.deepEqual(offsetTicks(axis, 2).map(tickHour), [10, 12]);
});

test("Not recorded gaps: 5 minutes or more between the cards of a day", () => {
  const d = "2026-10-12";
  const [col] = weekColumns([d], [[card(d, 9, 0, 9, 30), card(d, 9, 33, 10, 0), card(d, 10, 10, 11, 0)]]);
  assert.deepEqual(columnGaps(col), [[6 * H, 6 * H + 10 * 60]]);
});

test("arrow keys move within a day and to the nearest card on other days", () => {
  const days = weekDays("2026-10-16");
  const cols = weekColumns(days, [
    [card(days[0], 9, 0, 10, 0), card(days[0], 14, 0, 15, 0)],
    [],
    [card(days[2], 8, 0, 9, 0), card(days[2], 13, 30, 14, 0), card(days[2], 20, 0, 21, 0)],
    [],
    [],
    [],
    [],
  ]);
  assert.deepEqual(moveInWeek(cols, { col: 0, i: 0 }, "ArrowDown"), { col: 0, i: 1 });
  assert.equal(moveInWeek(cols, { col: 0, i: 1 }, "ArrowDown"), null);
  assert.equal(moveInWeek(cols, { col: 0, i: 0 }, "ArrowUp"), null);
  // Monday 2 PM → right skips the empty Tuesday → Wednesday 1:30 PM (nearest in time).
  assert.deepEqual(moveInWeek(cols, { col: 0, i: 1 }, "ArrowRight"), { col: 2, i: 1 });
  assert.deepEqual(moveInWeek(cols, { col: 2, i: 2 }, "ArrowLeft"), { col: 0, i: 1 });
  assert.equal(moveInWeek(cols, { col: 2, i: 0 }, "ArrowRight"), null);
  assert.deepEqual(moveInWeek(cols, { col: 2, i: 0 }, "End"), { col: 2, i: 2 });
  assert.deepEqual(moveInWeek(cols, { col: 2, i: 2 }, "Home"), { col: 2, i: 0 });
  // The tab stop starts on the shared day, else on the nearest day with cards.
  assert.deepEqual(firstStop(cols, days, days[2]), { col: 2, i: 0 });
  assert.deepEqual(firstStop(cols, days, days[1]), { col: 0, i: 0 });
  assert.deepEqual(firstStop(cols, days, days[5]), { col: 2, i: 0 });
});

test("review walks the cards that are not failed and less than 80 % rated, by start time", () => {
  const d = "2026-10-16";
  const a = card(d, 9, 0, 9, 30, "Work", { id: 1 });
  const b = card(d, 8, 0, 8, 30, "Work", { id: 2 });
  const failed = card(d, 10, 0, 10, 15, "System", { id: 3 });
  const idle = card(d, 11, 0, 11, 15, "Idle", { id: 4, isIdleShortcut: true });
  const rated = card(d, 12, 0, 12, 15, "Work", { id: 5 });
  const partly = card(d, 13, 0, 13, 15, "Work", { id: 6 });
  const q = reviewQueue([a, b, failed, idle, rated, partly], new Map([[5, 0.8], [6, 0.5]]));
  assert.deepEqual(q.map((c) => c.id), [2, 1, 4, 6]);
});
