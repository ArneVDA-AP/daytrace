// Run: node --experimental-strip-types --test src/lib/time.test.mjs src/lib/keys.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { parseDayQuery } from "./time.ts";

const TODAY = "2026-10-16"; // a Friday

test("words for today and yesterday", () => {
  assert.equal(parseDayQuery("today", TODAY), "2026-10-16");
  assert.equal(parseDayQuery(" Yesterday ", TODAY), "2026-10-15");
  assert.equal(parseDayQuery("gisteren", TODAY), "2026-10-15");
});

test("weekday names go back to the most recent one", () => {
  assert.equal(parseDayQuery("monday", TODAY), "2026-10-12");
  assert.equal(parseDayQuery("Mon", TODAY), "2026-10-12");
  assert.equal(parseDayQuery("friday", TODAY), "2026-10-16");
  assert.equal(parseDayQuery("last friday", TODAY), "2026-10-09");
  assert.equal(parseDayQuery("thurs", TODAY), "2026-10-15");
  assert.equal(parseDayQuery("saturday", TODAY), "2026-10-10");
  assert.equal(parseDayQuery("maandag", TODAY), "2026-10-12");
  assert.equal(parseDayQuery("t", TODAY), null);
});

test("day and month in either order, with or without a year", () => {
  assert.equal(parseDayQuery("12 oct", TODAY), "2026-10-12");
  assert.equal(parseDayQuery("oct 12", TODAY), "2026-10-12");
  assert.equal(parseDayQuery("12th October", TODAY), "2026-10-12");
  assert.equal(parseDayQuery("3 okt", TODAY), "2026-10-03");
  assert.equal(parseDayQuery("fri 9 oct", TODAY), "2026-10-09");
  assert.equal(parseDayQuery("20 dec", TODAY), "2025-12-20", "a date still to come this year means last year");
  assert.equal(parseDayQuery("12 october 2025", TODAY), "2025-10-12");
  assert.equal(parseDayQuery("30 feb", TODAY), null);
});

test("numeric dates are day/month, and ISO dates", () => {
  assert.equal(parseDayQuery("12/10", TODAY), "2026-10-12");
  assert.equal(parseDayQuery("1.9", TODAY), "2026-09-01");
  assert.equal(parseDayQuery("12-10-2025", TODAY), "2025-10-12");
  assert.equal(parseDayQuery("2026-10-12", TODAY), "2026-10-12");
  assert.equal(parseDayQuery("2026-10-20", TODAY), null, "after today");
  assert.equal(parseDayQuery("2026-02-30", TODAY), null);
});

test("a bare day of the month", () => {
  assert.equal(parseDayQuery("12", TODAY), "2026-10-12");
  assert.equal(parseDayQuery("20", TODAY), "2026-09-20");
  assert.equal(parseDayQuery("31", TODAY), null, "no 31 September");
});

test("anything else is not a day", () => {
  assert.equal(parseDayQuery("settings", TODAY), null);
  assert.equal(parseDayQuery("export", TODAY), null);
  assert.equal(parseDayQuery("", TODAY), null);
  assert.equal(parseDayQuery("pause 15", TODAY), null);
});
