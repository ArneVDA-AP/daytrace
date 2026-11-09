// Release checks before a `v*` tag:
//   1. the version is the same in package.json, src-tauri/tauri.conf.json, src-tauri/Cargo.toml
//      and the daytrace package in src-tauri/Cargo.lock;
//   2. CHANGELOG.md has a non-empty section for that version (what scripts/changelog-section.mjs
//      prints as the release notes);
//   3. it lists the README and docs lines that still say there is no release (they change at the
//      release; this check reports them and does not fail on them).
//
//   node scripts/check-release.mjs [repo-root]
//
// Exits 1 when check 1 or 2 fails.
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { execFileSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const root = resolve(process.argv[2] ?? join(fileURLToPath(import.meta.url), "..", ".."));
const read = (p) => readFileSync(join(root, p), "utf8").replace(/\r\n?/g, "\n");
let failed = false;

// 1. Version sync.
const versions = {
  "package.json": JSON.parse(read("package.json")).version,
  "src-tauri/tauri.conf.json": JSON.parse(read("src-tauri/tauri.conf.json")).version,
  "src-tauri/Cargo.toml": read("src-tauri/Cargo.toml").match(/^\[package\][^[]*?^version\s*=\s*"([^"]+)"/m)?.[1],
  "src-tauri/Cargo.lock": read("src-tauri/Cargo.lock").match(/^name = "daytrace"\nversion = "([^"]+)"/m)?.[1],
};
for (const [file, v] of Object.entries(versions)) console.log(`version ${file}: ${v ?? "(not found)"}`);
const distinct = new Set(Object.values(versions));
const version = versions["package.json"];
if (distinct.size !== 1 || !version) {
  console.log("FAIL version: the files do not agree");
  failed = true;
} else {
  console.log(`PASS version: ${version} everywhere`);
}

// 2. Changelog section. The version's own section must have text: changelog-section.mjs (which the
// release workflow runs) falls back to "Unreleased" when it is empty or missing.
if (version) {
  const lines = read("CHANGELOG.md").split("\n");
  const name = (l) => l.slice(3).trim().split(/\s+/)[0].replace(/^\[|\]$/g, "").replace(/^v/i, "");
  const at = lines.findIndex((l) => l.startsWith("## ") && name(l) === version);
  const next = at < 0 ? -1 : lines.findIndex((l, i) => i > at && l.startsWith("## "));
  const own = at < 0 ? "" : lines.slice(at + 1, next < 0 ? undefined : next).join("\n").trim();
  const notes = execFileSync(process.execPath, [join(root, "scripts", "changelog-section.mjs"), `v${version}`], { encoding: "utf8" }).trim();
  // A line is a release note when it is not a Markdown heading or a link reference definition (which
  // also covers the "[//]: # (...)" and "[//]: <> (...)" comment idioms) and still has a letter or digit
  // once HTML comments (also an unclosed one, and the short "<!-->" and "<!--->"), tags and entities are removed. So "---", "- ", "<br>" or
  // "&nbsp;" alone do not count, and "#123 fixed ..." does. A code fence line (``` or ~~~) is not a note;
  // the lines inside a fence count by their text. Code spans with a visible character are kept as text
  // first, so a note that shows `<!--` in backticks does not hide the notes after it; a code span that
  // holds only spaces is not text.
  const linkDefinition = /^\[[^\]]+\]:\s*(<[^>]*>|\S+)(\s+("[^"]*"|'[^']*'|\([^)]*\)))?\s*$/;
  const isNote = (l) => {
    const t = l.trim();
    if (/^#{1,6}(\s|$)/.test(t) || linkDefinition.test(t) || /^(`{3,}|~{3,})/.test(t)) return false;
    return /[\p{L}\p{N}]/u.test(t.replace(/<[^>]*>/g, "").replace(/&[#\w]+;/g, ""));
  };
  const text = own.replace(/`[^`\n]*[^`\s][^`\n]*`/g, "code").replace(/<!--(>|->|[\s\S]*?-->)/g, "").replace(/<!--[\s\S]*$/, "");
  if (at < 0) {
    console.log(`FAIL changelog: no "## ${version}" heading; the notes would come from "Unreleased"`);
    failed = true;
  } else if (!text.split("\n").some(isNote)) {
    console.log(`FAIL changelog: "## ${version}" is empty (or holds only headings, comments or markup)`);
    failed = true;
  } else if (notes !== own) {
    console.log(`FAIL changelog: changelog-section.mjs does not print the "## ${version}" section`);
    failed = true;
  } else {
    console.log(`PASS changelog: "## ${version}" has ${own.split("\n").length} lines of release notes`);
  }
}

// 3. Lines that must change once a release is published (reported, not failed).
const patterns = [/no published release yet/i, /not code-signed/i, /once a release exists/i, /after the first release/i];
for (const file of ["README.md", "docs/features.md"]) {
  read(file).split("\n").forEach((line, i) => {
    if (patterns.some((p) => p.test(line))) console.log(`REVIEW ${file}:${i + 1}: ${line.slice(0, 110)}${line.length > 110 ? "…" : ""}`);
  });
}

process.exit(failed ? 1 : 0);
