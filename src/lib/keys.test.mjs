// Run: node --experimental-strip-types --test src/lib/time.test.mjs src/lib/keys.test.mjs
import { test } from "node:test";
import assert from "node:assert/strict";
import { ariaKey, digitOf, isKey, keyLabel, modsAre } from "./keys.ts";

const ev = (code, m = {}) => ({ code, ctrlKey: !!m.ctrl, shiftKey: !!m.shift, altKey: !!m.alt, metaKey: !!m.meta });

test("Ctrl+1 on Belgian AZERTY: e.key is '&' but the code is Digit1", () => {
  assert.equal(digitOf(ev("Digit1", { ctrl: true })), 1);
  assert.equal(digitOf(ev("Numpad5")), 5);
  assert.equal(digitOf(ev("KeyA")), null);
});

test("modifiers must match exactly", () => {
  assert.ok(isKey(ev("KeyK", { ctrl: true }), "KeyK", { ctrl: true }));
  assert.ok(!isKey(ev("KeyK", { ctrl: true, alt: true }), "KeyK", { ctrl: true }), "AltGr reads as Ctrl+Alt");
  assert.ok(!isKey(ev("KeyJ", { meta: true }), "KeyJ"));
  assert.ok(modsAre(ev("BracketLeft")));
});

test("labels and aria names fall back to the US layout", () => {
  assert.equal(keyLabel("BracketLeft"), "[");
  assert.equal(keyLabel("KeyJ"), "J");
  assert.equal(ariaKey("Digit3", { ctrl: true }), "Control+3");
  assert.equal(ariaKey("KeyK", { ctrl: true }), "Control+K");
  assert.equal(ariaKey("BracketRight"), "]");
});
