// Run: node --experimental-strip-types --test src/app/settings-retry.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { isStaleSettings, retryStale, SAVE_TRIES } from "./settings-retry.ts";

const stale = (revision) => ({ code: "settings_changed", message: "The settings changed while this change was being saved. Try again.", revision });

/** A fake backend: get returns the settings with their revision; save refuses an old revision. */
function backend() {
  const db = { settings: { categories: ["Work"], retentionDays: 7 }, revision: 1 };
  return {
    db,
    get: async () => ({ settings: structuredClone(db.settings), revision: db.revision }),
    save: async (settings, base) => {
      if (base !== undefined && base !== db.revision) throw stale(db.revision);
      db.settings = settings;
      db.revision += 1;
      return { settings, revision: db.revision };
    },
  };
}

test("recognizes only the stale-settings error", () => {
  assert.ok(isStaleSettings(stale(3)));
  assert.ok(!isStaleSettings("The folder does not exist"));
  assert.ok(!isStaleSettings(new Error("x")));
  assert.ok(!isStaleSettings(null));
  assert.ok(!isStaleSettings({ code: "other" }));
});

test("a change made between read and save is kept: the patch is applied again", async () => {
  const b = backend();
  let reads = 0;
  const out = await retryStale(async () => {
    const dto = await b.get();
    reads += 1;
    // An AI tool adds a category right after the first read.
    if (reads === 1) {
      b.db.settings = { ...b.db.settings, categories: [...b.db.settings.categories, "Reading"] };
      b.db.revision += 1;
    }
    return b.save({ ...dto.settings, retentionDays: 3 }, dto.revision);
  });
  assert.equal(reads, 2);
  assert.deepEqual(out.settings, { categories: ["Work", "Reading"], retentionDays: 3 });
  assert.equal(b.db.revision, 3);
});

test("gives up after a bounded number of tries with a readable error", async () => {
  let tries = 0;
  await assert.rejects(
    retryStale(async () => {
      tries += 1;
      throw stale(tries);
    }),
    (e) => e instanceof Error && e.message.startsWith("The settings changed"),
  );
  assert.equal(tries, SAVE_TRIES);
});

test("other errors are passed on at once", async () => {
  let tries = 0;
  await assert.rejects(
    retryStale(async () => {
      tries += 1;
      throw "The recap folder does not exist.";
    }),
    (e) => e === "The recap folder does not exist.",
  );
  assert.equal(tries, 1);
});

test("a save without a revision is not checked (callers from before revisions)", async () => {
  const b = backend();
  b.db.revision = 9;
  const out = await retryStale(() => b.save({ categories: [], retentionDays: 1 }, undefined));
  assert.equal(out.revision, 10);
});
