// Prints the CHANGELOG.md section of a version, for the GitHub release notes (build.yml).
// Daytrace reads the same section when it shows "What's new" offline
// (src-tauri/src/features/platform_extras/whats_new.rs).
//
//   node scripts/changelog-section.mjs 0.3.0      (a leading "v" is fine)
//
// Takes the `## 0.3.0` (or `## [0.3.0] - date`) section; when there is none, the `## Unreleased`
// section; otherwise prints nothing.
import { readFileSync } from "node:fs";

const want = (process.argv[2] ?? "").trim().replace(/^v/i, "");
const text = readFileSync(new URL("../CHANGELOG.md", import.meta.url), "utf8").replace(/\r\n?/g, "\n");

function section(match) {
  const out = [];
  let inside = false;
  for (const line of text.split("\n")) {
    if (line.startsWith("## ")) {
      if (inside) break;
      inside = match(line.slice(3).trim().split(/\s+/)[0].replace(/^\[|\]$/g, ""));
      continue;
    }
    if (inside) out.push(line);
  }
  return out.join("\n").trim();
}

const body = (want && section((v) => v.replace(/^v/i, "") === want)) || section((v) => v.toLowerCase() === "unreleased");
if (body) process.stdout.write(body + "\n");
