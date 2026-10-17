// Post-build checks for the Tauri bundle (runs as part of `npm run build`, so CI runs it too).
//
// 1. CSP: dist/index.html must not contain an inline <style> or inline <script>. Tauri adds a
//    nonce to style-src as soon as the built index.html has an inline <style> (tauri
//    manager/mod.rs replace_csp_nonce); with a nonce present the browser ignores 'unsafe-inline',
//    and every <style> injected at runtime (Radix Dialog's scroll lock, for one) stops applying.
//    Inline scripts are blocked outright (script-src falls back to 'self').
// 2. The browser-preview IPC mock (src/dev) must not be in the production bundle.
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const dist = new URL("../dist/", import.meta.url).pathname;
const problems = [];

const html = readFileSync(join(dist, "index.html"), "utf8");
if (/<style[\s>]/i.test(html)) problems.push("dist/index.html contains an inline <style>. Move it into a stylesheet (src/index.css).");
for (const m of html.matchAll(/<script\b([^>]*)>([\s\S]*?)<\/script>/gi)) {
  if (!/\bsrc=/.test(m[1]) || m[2].trim()) problems.push("dist/index.html contains an inline <script>; Tauri's CSP blocks it.");
}

const MARKER = "DAYTRACE_DEV_MOCK_IPC";
const walk = (dir) =>
  readdirSync(dir).flatMap((f) => {
    const p = join(dir, f);
    return statSync(p).isDirectory() ? walk(p) : [p];
  });
for (const f of walk(dist)) {
  if (!/\.(js|css|html)$/.test(f)) continue;
  if (readFileSync(f, "utf8").includes(MARKER)) problems.push(`${f.slice(dist.length)} contains the dev IPC mock (${MARKER}); it must stay out of production builds.`);
}

if (problems.length) {
  console.error("check-dist failed:\n- " + problems.join("\n- "));
  process.exit(1);
}
console.log("check-dist: index.html has no inline <style>/<script>; no dev mock in dist.");
