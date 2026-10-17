// Run: node --experimental-strip-types --test src/views/rollover.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { rolloverDay, shouldIdleReset, IDLE_RESET_MS } from "./rollover.ts";

test("moves from yesterday's today to the new day at 4 AM", () => {
  assert.equal(rolloverDay("2026-10-17", "2026-10-17", "2026-10-18"), "2026-10-18");
});

test("keeps an older day the user picked", () => {
  assert.equal(rolloverDay("2026-10-12", "2026-10-17", "2026-10-18"), "2026-10-12");
});

test("does nothing without a day change or before the first status", () => {
  assert.equal(rolloverDay("2026-10-17", "2026-10-17", "2026-10-17"), "2026-10-17");
  assert.equal(rolloverDay("2026-10-17", null, "2026-10-18"), "2026-10-17");
});

test("idle reset after 15 minutes, once per idle stretch", () => {
  const t0 = 1_000_000;
  assert.equal(shouldIdleReset(t0 + IDLE_RESET_MS - 1, t0, null), false);
  assert.equal(shouldIdleReset(t0 + IDLE_RESET_MS, t0, null), true);
  // Already reset during this stretch.
  assert.equal(shouldIdleReset(t0 + 2 * IDLE_RESET_MS, t0, t0 + IDLE_RESET_MS), false);
  // New interaction, then idle again.
  const t1 = t0 + 3 * IDLE_RESET_MS;
  assert.equal(shouldIdleReset(t1 + IDLE_RESET_MS, t1, t0 + IDLE_RESET_MS), true);
});
