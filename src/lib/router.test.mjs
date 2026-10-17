// Run: node --experimental-strip-types --test src/lib/router.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { dayParam, isDayKey } from "./router.ts";

test("only real calendar dates are days", () => {
  assert.ok(isDayKey("2026-10-16"));
  assert.ok(isDayKey("2028-02-29"));
  assert.ok(!isDayKey("2026-02-30"), "30 February would show as 2 March");
  assert.ok(!isDayKey("2026-13-01"));
  assert.ok(!isDayKey("2026-1-01"));
  assert.ok(!isDayKey("tomorrow"));
});

test("dayParam drops invalid days", () => {
  assert.equal(dayParam(new URLSearchParams("day=2026-10-16")), "2026-10-16");
  assert.equal(dayParam(new URLSearchParams("day=2026-02-30")), null);
  assert.equal(dayParam(new URLSearchParams("")), null);
});
