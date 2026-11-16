// Run: node --experimental-strip-types --test src/views/timeline/session-card-text.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { conditionText, folderName, sessionCardCount } from "./session-card-text.ts";

test("a 'fails' condition that is met did not happen; one that failed happened", () => {
  assert.equal(conditionText("must", "met"), "Shown");
  assert.equal(conditionText("must", "failed"), "Not shown");
  assert.equal(conditionText("fails", "met"), "Did not happen");
  assert.equal(conditionText("fails", "failed"), "Happened");
});

test("a condition that was not run, or has a state this build does not know, is not checked", () => {
  assert.equal(conditionText("must", "open"), "Not checked");
  assert.equal(conditionText("fails", "open"), "Not checked");
  assert.equal(conditionText("must", ""), "Not checked");
  assert.equal(conditionText("fails", "skipped"), "Not checked");
});

test("the folder label is the last part of a Windows or POSIX path", () => {
  assert.equal(folderName("C:\\dev\\notes-app"), "notes-app");
  assert.equal(folderName("C:\\dev\\notes-app\\"), "notes-app");
  assert.equal(folderName("/home/me/projects/site"), "site");
  assert.equal(folderName("C:\\"), "C:");
  assert.equal(folderName(""), "");
});

test("the count names one card or several", () => {
  assert.equal(sessionCardCount(1), "1 session card");
  assert.equal(sessionCardCount(3), "3 session cards");
});
