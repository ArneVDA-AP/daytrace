// Run: node --experimental-strip-types --test src/app/failure.test.mjs
// The same cases as src-tauri/src/llm/failure.rs tests, plus the UI wording.
import { test } from "node:test";
import assert from "node:assert/strict";
import { classifyFailure, describeFailure, isUnreachable } from "./failure.ts";

test("classifies like failure.rs", () => {
  assert.deepEqual(classifyFailure("Claude Code CLI: Invalid API key · Please run /login"), { kind: "loginExpired", provider: "Claude" });
  assert.equal(classifyFailure("Claude Code CLI: You've hit your limit · resets 3pm (Europe/Brussels)").kind, "usageLimit");
  assert.equal(classifyFailure("Claude Code CLI not found. Install it (in PowerShell: ...)").kind, "cliMissing");
  assert.equal(classifyFailure("Codex CLI: ERROR: You've hit your usage limit.").provider, "ChatGPT");
  assert.equal(classifyFailure("CLI process timed out after 300 seconds").kind, "transient");
  assert.equal(classifyFailure("Card 1 'Reading about usage limit docs' is only 8.0 minutes long").kind, "unreadable", "validation wins over words in titles");
  assert.equal(classifyFailure('{"cards": [').kind, "unreadable");
  assert.equal(classifyFailure("").kind, "unreadable");
  assert.equal(classifyFailure('HTTP 401: {"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}').kind, "apiKey");
  assert.equal(classifyFailure("HTTP 529: overloaded").kind, "transient");
  assert.equal(classifyFailure("Could not reach the model server: error sending request").kind, "localOffline");
  assert.equal(classifyFailure("something odd").kind, "unknown");
  assert.equal(classifyFailure('HTTP 400: {"error":{"message":"Your credit balance is too low to access the Anthropic API."}}').kind, "noCredits");
});

test("network failures go to the Can't reach banner, the rest to the failure banner", () => {
  assert.ok(isUnreachable("error sending request for url (https://api.anthropic.com/v1/messages): operation timed out", "anthropic"));
  assert.ok(isUnreachable("Could not reach the model server: error sending request", "ollama"));
  assert.ok(!isUnreachable("HTTP 429: rate limit exceeded", "anthropic"), "busy is not unreachable");
  assert.ok(!isUnreachable('HTTP 401: invalid x-api-key', "anthropic"));
  assert.ok(!isUnreachable("CLI process timed out after 300 seconds", "claude_cli"), "a slow CLI is not unreachable");
  assert.ok(!isUnreachable(null));
});

const ctx = { provider: "anthropic", recording: true };

test("each cause has its own fix", () => {
  const key = describeFailure('HTTP 401: {"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}', ctx);
  assert.equal(key.label, "Claude refused the API key");
  assert.deepEqual(key.fixes.map((f) => f.label), ["Replace API key", "Open Claude Console"]);
  assert.ok(!key.selfHealing);

  const credits = describeFailure("HTTP 400: Your credit balance is too low", ctx);
  assert.equal(credits.fixes[0].label, "Open Claude Console");
  assert.equal(credits.fixes[0].kind, "link");

  const busy = describeFailure("HTTP 429: rate limit exceeded", ctx);
  assert.equal(busy.label, "Claude is busy");
  assert.ok(busy.selfHealing);
  assert.deepEqual(busy.fixes.map((f) => f.label), ["Try again"]);
  assert.match(busy.body, /tries again by itself with the next batch/);

  const flaky = describeFailure("The model's answer could not be read: failed to parse", { ...ctx, recording: false });
  assert.equal(flaky.kind, "unreadable");
  assert.match(flaky.body, /once recording resumes/);
  assert.equal(flaky.next, "Tries again when recording resumes");
});

test("CLI providers are named by the tool", () => {
  const out = describeFailure("Claude Code CLI: Not logged in · Please run /login", { provider: "claude_cli", recording: true });
  assert.equal(out.label, "Claude Code is signed out");
  assert.match(out.body, /run “claude”, type “\/login”/);
  const slow = describeFailure("CLI process timed out after 300 seconds", { provider: "claude_cli", recording: true });
  assert.equal(slow.label, "Claude Code took too long to answer");
  const codex = describeFailure("Codex CLI: ERROR: You've hit your usage limit.", { provider: "codex_cli", recording: true });
  assert.equal(codex.label, "Codex usage limit reached");
});
