/*
 * DEV ONLY: platform fixtures of wave 4 (update install, What's new, Copy debug info) for the
 * browser preview; src/dev/mock-ipc.ts merges these handlers over its own. SYNTHETIC data only.
 *
 * Variants, as `?f4=` before the hash (comma-separated):
 *   whatsnew       "What's new" opens with release notes from GitHub
 *   whatsnew-offline   the same from the bundled CHANGELOG
 *   downloading    an update is downloading (42%)
 *   ready          an update is downloaded and waits for the PC to be idle
 *   installfail    the download failed (no signed update in the release)
 *   debugfail      Copy debug info fails
 *   batchapi       the reprocess dialog says the requests skip the Batch API
 * Example: http://localhost:1420/?f4=ready&settings=update#/settings?section=about
 */
import type { InstallStatus, WhatsNew } from "@/views/platform-api";

type Args = Record<string, unknown>;

const NOTES = `### Added
- Pause and resume from a script: \`daytrace.exe --pause 30\` and \`daytrace.exe --resume\`.
- Updates install by themselves while the PC is idle or locked.
- **Copy debug info** in Settings.

### Fixed
- Reprocessing a day with the Batch API on finishes in minutes. See [the release page](https://github.com/ArneVDA-AP/daytrace/releases).`;

const DEBUG = `# Daytrace debug info
Daytrace 0.3.0 · windows x86_64 · made 2026-10-16 17:42:00 (+02:00)

## Settings
Provider: Claude API · model claude-haiku-5-5 · https://api.anthropic.com
Mode: Cloud · Batch API on · reasoning Low · language default

## Last 20 batches
id | start – end | status | reason | tries
812 | 2026-10-16 17:15:00 – 17:30:00 | completed |  | 0
`;

export function applyF4Mock(base: Record<string, (a: Args) => unknown>, ctx: { q: URLSearchParams; now: () => number }) {
  const v = new Set((ctx.q.get("f4") ?? "").split(",").filter(Boolean));
  const install = (): InstallStatus => {
    const idle: InstallStatus = { stage: "idle", version: null, downloaded: 0, total: null, error: null, failedAt: null };
    if (v.has("downloading")) return { ...idle, stage: "downloading", version: "0.3.0", downloaded: 5_100_000, total: 12_200_000 };
    if (v.has("ready")) return { ...idle, stage: "ready", version: "0.3.0", downloaded: 12_200_000, total: 12_200_000 };
    if (v.has("installfail"))
      return { ...idle, stage: "failed", version: "0.3.0", error: "This release has no signed update for Daytrace to install. Use the release page.", failedAt: ctx.now() - 600 };
    return idle;
  };
  let whatsNew: WhatsNew | null = v.has("whatsnew")
    ? { version: "0.3.0", notes: NOTES, source: "release", releaseUrl: "https://github.com/ArneVDA-AP/daytrace/releases/tag/v0.3.0" }
    : v.has("whatsnew-offline")
      ? { version: "0.3.0", notes: NOTES, source: "changelog", releaseUrl: null }
      : null;
  const info = base.platform_info;
  const plan = base.te_reprocess_day_plan;
  if (plan && v.has("batchapi"))
    base.te_reprocess_day_plan = (a: Args) => ({
      ...(plan(a) as object),
      costNote: "With the Batch API on, these requests are sent directly so the day is done in minutes instead of hours. They cost the regular price, twice the Batch API price.",
    });
  Object.assign(base, {
    platform_info: (a: Args) => ({ ...(info(a) as object), install: install() }),
    platform_install_update: () => {
      throw "The update is no longer offered.";
    },
    platform_whats_new: () => whatsNew,
    platform_whats_new_seen: () => {
      whatsNew = null;
      return null;
    },
    platform_debug_info: () => {
      if (v.has("debugfail")) throw "database is locked";
      return DEBUG;
    },
    platform_open_link: () => null,
  });
}
