// Run: node --experimental-strip-types --test src/views/timeline/session-card-text.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { conditionText, drawingAlt, folderName, sessionCardCount, svgDataUrl } from "./session-card-text.ts";

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

test("a drawing becomes an image address that holds every character of it", () => {
  const svg = '<svg xmlns="http://www.w3.org/2000/svg"><style>a{fill:#fff}</style><text>50% "done" & café</text></svg>';
  const url = svgDataUrl(svg);
  assert.ok(url.startsWith("data:image/svg+xml;charset=utf-8,"));
  // Nothing that ends or splits a URL is left raw: no quote, bracket, hash or space.
  assert.doesNotMatch(url.slice(url.indexOf(",") + 1), /["<>#\s]/);
  assert.equal(decodeURIComponent(url.slice(url.indexOf(",") + 1)), svg);
});

test("the words for a drawing give the grade, the outcome and the grade note", () => {
  assert.equal(drawingAlt("Pass", "The export works.", "42 tests ran."), "Session card, pass: The export works. 42 tests ran.");
  assert.equal(drawingAlt("Fail", "The export works.", ""), "Session card, fail: The export works.");
});

test("the count names one card or several", () => {
  assert.equal(sessionCardCount(1), "1 session card");
  assert.equal(sessionCardCount(3), "3 session cards");
});
