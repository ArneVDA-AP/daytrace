/*
 * DEV ONLY: Agents section fixtures for the browser preview (src/dev/mock-ipc.ts merges these
 * handlers over its own). SYNTHETIC data only. The "dashboard" is a static placeholder page
 * (src/dev/agents-preview.html) served by the dev server; nothing is started.
 *
 * Variants, as `?agents=` before the hash:
 *   (none)      Node.js found, stopped; Start → starting → running after 1.5 s
 *   running     already running
 *   starting    stays starting
 *   failed      the last start failed (download error, with log lines)
 *   startfail   Start → starting → failed after 1.5 s
 *   nonode      Node.js not found
 *   oldnode     Node.js 18 found (too old)
 *   statuserror agents_status fails
 * Example: http://localhost:1420/?agents=nonode#/agents
 */

type Args = Record<string, unknown>;

const VERSION = "1.2.1";
const NODE_MISSING = "Node.js was not found. AgentPlayback runs on Node.js 20 or later with npm.";

export function agentsMock(q: URLSearchParams, now: () => number): Record<string, (a: Args) => unknown> {
  const variant = q.get("agents") ?? "";
  const previewUrl = `${location.origin}/src/dev/agents-preview.html`;
  const failed = {
    state: "failed",
    title: "AgentPlayback didn’t start",
    message: "npx couldn’t download AgentPlayback. Check the internet connection and try again.",
    log: [
      "npm error code ENOTFOUND",
      "npm error syscall getaddrinfo",
      "npm error errno ENOTFOUND",
      "npm error network request to https://registry.npmjs.org/agentplayback failed, reason: getaddrinfo ENOTFOUND registry.npmjs.org",
      "npm error A complete log of this run can be found in: C:\\Users\\Preview\\AppData\\Local\\npm-cache\\_logs\\2026-10-16T15_40_02_118Z-debug-0.log",
    ],
  };
  let run: Record<string, unknown> =
    variant === "running"
      ? { state: "running", url: previewUrl, since: now() - 1800 }
      : variant === "starting"
        ? { state: "starting", since: now() - 20 }
        : variant === "failed"
          ? failed
          : variant === "nonode"
            ? { state: "notInstalled", reason: NODE_MISSING }
            : variant === "oldnode"
              ? { state: "notInstalled", reason: "Node.js v18.19.1 is installed, but AgentPlayback needs Node.js 20 or later." }
              : { state: "stopped" };
  let timer: ReturnType<typeof setTimeout> | null = null;

  const node =
    variant === "nonode"
      ? null
      : variant === "oldnode"
        ? null
        : { version: "v22.11.0", nodePath: "C:\\Program Files\\nodejs\\node.exe", npxPath: "C:\\Program Files\\nodejs\\npx.cmd" };

  const status = () => {
    if (variant === "statuserror") throw "The agents state could not be read (synthetic error).";
    return {
      ...structuredClone(run),
      node,
      version: VERSION,
      dataDir: "C:\\Users\\Preview\\AppData\\Roaming\\dev.daytrace.app\\agents",
      sources: [
        { agent: "Claude Code", path: "C:\\Users\\Preview\\.claude\\projects", exists: true },
        { agent: "Codex", path: "C:\\Users\\Preview\\.codex", exists: false },
      ],
    };
  };

  return {
    agents_status: status,
    agents_recheck: status,
    agents_start: () => {
      if (run.state === "notInstalled") return status();
      if (run.state !== "starting" && run.state !== "running") {
        run = { state: "starting", since: now() };
        if (timer) clearTimeout(timer);
        timer = setTimeout(() => {
          run = variant === "startfail" ? failed : { state: "running", url: previewUrl, since: now() };
        }, 1500);
      }
      return status();
    },
    agents_stop: () => {
      if (timer) clearTimeout(timer);
      timer = null;
      if (run.state !== "notInstalled") run = { state: "stopped" };
      return status();
    },
    agents_open_in_browser: () => {
      if (run.state !== "running") throw "AgentPlayback is not running.";
      return null;
    },
  };
}
