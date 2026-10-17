#!/usr/bin/env python3
"""Screenshots of the browser preview (Vite dev server + the mocked IPC in src/dev).

    npm run screenshots -- --out /path/to/folder                  # the default route list below
    npm run screenshots -- --out DIR --only timeline              # names containing "timeline"
    npm run screenshots -- --out DIR --route daily-empty='/?empty=1#/daily'
    npm run screenshots -- --out DIR --widths 1280,760            # every route at both widths
    npm run screenshots -- --out DIR --only settings --forced     # Windows Contrast themes
    npm run screenshots -- --out DIR --url http://localhost:1420  # use a dev server you started

Writes <out>/<name>-<theme>[-<width>][-forced].png (the width only when it is not 1280; a route's
own w= option counts too). Uses Playwright's Chromium from PLAYWRIGHT_BROWSERS_PATH (default
/opt/pw-browsers); it never downloads browsers.

The clock is fixed (default Friday 16 October 2026, 5:42 PM local) so the synthetic data and the
"now" line look the same on every run; --time now uses the real clock. Segoe UI Variable is not on
Linux: the harness maps each weight of it to the matching Inter face if Inter is installed (the
app itself never ships Inter). --forced emulates forced-colors: active (a Contrast theme; with
--themes dark it is a dark Contrast theme).
Route options after a '|': w=760 (width), h=900 (height), keys=Control+k (press keys after load),
click=<css selector>, wait=800 (extra ms), full=1 (full page).
"""
import argparse
import os
import signal
import socket
import subprocess
import sys
import time
import urllib.request
from datetime import datetime

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DAY = "2026-10-16"
PREV = "2026-10-15"

# (name, path after the origin, options). Names: <section>-<state>.
ROUTES = [
    ("timeline-day-so-far", f"/#/timeline?day={DAY}", ""),
    ("timeline-card", f"/#/timeline?day={DAY}&card=4", ""),
    ("timeline-card-failed", f"/#/timeline?day={DAY}&card=8", ""),
    ("timeline-card-idle", f"/#/timeline?day={DAY}&card=6", ""),
    ("timeline-night-owl", f"/#/timeline?day={PREV}", ""),
    ("timeline-no-data", "/#/timeline?day=2026-10-14", ""),
    ("timeline-empty-install", "/?empty=1#/timeline", ""),
    ("timeline-error", f"/?tl=dayerror#/timeline?day={DAY}", ""),
    ("timeline-export", f"/#/timeline?day={DAY}&export=1", "wait=500"),
    ("timeline-list", f"/#/timeline?day={DAY}", "w=760"),
    ("timeline-card", f"/#/timeline?day={DAY}&card=4", "w=760"),
    ("timeline-week", f"/#/timeline?day={DAY}&mode=week", ""),
    ("timeline-week", f"/#/timeline?day={DAY}&mode=week", "w=760"),
    ("timeline-week-empty", "/?empty=1#/timeline?mode=week", ""),
    ("timeline-review", f"/#/timeline?day={DAY}&review=1", "wait=600"),
    ("timeline-review", f"/#/timeline?day={DAY}&review=1", "w=760|wait=600"),
    ("daily-today", f"/#/daily?day={DAY}", ""),
    ("daily-generated", f"/#/daily?day={PREV}", ""),
    ("daily-generating", f"/?recap=generating#/daily?day={DAY}", ""),
    ("daily-failed", f"/?recap=failed#/daily?day={DAY}", ""),
    ("daily-no-data", "/#/daily?day=2026-10-14", ""),
    ("daily-error", f"/?insights=error#/daily?day={DAY}", ""),
    ("daily-today", f"/#/daily?day={DAY}", "w=760"),
    ("weekly-charts", f"/#/weekly?day={DAY}", ""),
    ("weekly-tables", f"/#/weekly?day={DAY}&view=table", ""),
    ("weekly-empty", "/?empty=1#/weekly", ""),
    ("weekly-error", f"/?insights=error#/weekly?day={DAY}", ""),
    ("weekly-charts", f"/#/weekly?day={DAY}", "w=760"),
    ("goals-today", f"/#/journal?day={DAY}", ""),
    ("goals-yesterday", f"/#/journal?day={PREV}", ""),
    ("goals-prompt", f"/?goals=prompt#/journal?day={DAY}", ""),
    ("goals-no-data", "/#/journal?day=2026-10-14", ""),
    ("goals-today", f"/#/journal?day={DAY}", "w=760"),
    ("chat-conversation", "/#/chat?c=1", ""),
    ("chat-empty", "/?chat=empty#/chat", ""),
    ("chat-error", "/?chat=error#/chat?c=1", ""),
    ("chat-conversation", "/#/chat?c=1", "w=760"),
    ("chat-charts", "/?chat=charts#/chat?c=91", "h=2600"),
    ("chat-charts-invalid", "/?chat=charts#/chat?c=92", "h=2000"),
    ("chat-charts", "/?chat=charts#/chat?c=91", "w=760|h=2700"),
    ("settings-provider", "/#/settings?section=provider", ""),
    ("settings-recording", "/#/settings?section=recording", ""),
    ("settings-categories", "/#/settings?section=categories", ""),
    ("settings-storage", "/#/settings?section=storage", ""),
    ("settings-appearance", "/#/settings?section=appearance", ""),
    ("settings-about", "/#/settings?section=about", ""),
    ("settings-provider", "/#/settings?section=provider", "w=760"),
    ("onboarding-welcome", "/?onboarding=1#/timeline", ""),
    ("onboarding-welcome", "/?onboarding=1#/timeline", "w=760"),
    ("shell-paused", f"/?status=paused-timed#/timeline?day={DAY}", ""),
    ("shell-writing-locked", f"/?status=locked,writing#/daily?day={DAY}", ""),
    ("shell-banner-nokey", f"/?status=nokey#/timeline?day={DAY}", ""),
    ("shell-banner-offline", f"/?status=offline#/weekly?day={DAY}", ""),
    ("shell-banner-capture", f"/?status=capture-error,offline#/journal?day={DAY}", "w=760"),
    ("shell-status-popover", f"/?status=offline#/timeline?day={DAY}", "w=760|click=[aria-label^='Status:']|wait=400"),
    ("shell-command-menu", f"/#/timeline?day={DAY}", "keys=Control+k|wait=600"),
    ("shell-day-picker", f"/#/daily?day={DAY}", "click=[aria-label='Go to a day']|wait=400"),
    ("shell-pause-menu", f"/#/chat", "click=[aria-label='Pause recording for…']|wait=400"),
    ("shell-paused", f"/?status=paused#/timeline?day={DAY}", "w=760"),
    ("shell-failure-key", f"/?fail=apikey#/timeline?day={DAY}", ""),
    ("shell-failure-busy", f"/?fail=busy#/weekly?day={DAY}", ""),
    ("shell-failure-signed-out", f"/?provider=claude_cli&fail=login#/daily?day={DAY}", "w=760"),
    ("shell-recap-badge", f"/?badge=recap#/timeline?day={DAY}", ""),
    ("shell-recap-badge", f"/?badge=recap#/timeline?day={DAY}", "w=760"),
]

# One face per weight, so semibold and bold render as Inter SemiBold and Bold, not synthesized.
FONT_CSS = "\n".join([
    '@font-face { font-family: "Segoe UI Variable Text"; src: local("Inter"), local("Inter Regular"); font-weight: 400; }',
    '@font-face { font-family: "Segoe UI Variable Text"; src: local("Inter Medium"); font-weight: 500; }',
    '@font-face { font-family: "Segoe UI Variable Text"; src: local("Inter SemiBold"); font-weight: 600; }',
    '@font-face { font-family: "Segoe UI Variable Text"; src: local("Inter Bold"); font-weight: 700; }',
    '@font-face { font-family: "Segoe UI Variable Display"; src: local("Inter Display"), local("Inter Display Regular"); font-weight: 400; }',
    '@font-face { font-family: "Segoe UI Variable Display"; src: local("Inter Display Medium"); font-weight: 500; }',
    '@font-face { font-family: "Segoe UI Variable Display"; src: local("Inter Display SemiBold"); font-weight: 600; }',
    '@font-face { font-family: "Segoe UI Variable Display"; src: local("Inter Display Bold"); font-weight: 700; }',
])


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    p = s.getsockname()[1]
    s.close()
    return p


def wait_http(url, timeout=40):
    end = time.time() + timeout
    while time.time() < end:
        try:
            urllib.request.urlopen(url, timeout=2)
            return True
        except Exception:
            time.sleep(0.3)
    return False


def stop(proc):
    try:
        os.killpg(proc.pid, signal.SIGTERM)
    except ProcessLookupError:
        pass
    proc.wait(timeout=10)


def parse_opts(s):
    out = {}
    for part in filter(None, s.split("|")):
        k, _, v = part.partition("=")
        out[k] = v
    return out


def shoot(browser, base, a, name, path, o, theme, width):
    ctx = browser.new_context(
        viewport={"width": width, "height": int(o.get("h", a.height))},
        color_scheme=theme, reduced_motion="reduce", device_scale_factor=1,
        forced_colors="active" if a.forced else "none",
    )
    page = ctx.new_page()
    page.on("pageerror", lambda e, n=name: print(f"  page error in {n}: {e}", file=sys.stderr))
    if a.time != "now":
        page.clock.set_fixed_time(datetime.fromisoformat(a.time))
    page.goto(base + path)
    page.add_style_tag(content=FONT_CSS)
    page.wait_for_selector("h1", timeout=15000)
    page.evaluate("document.fonts.ready")
    page.wait_for_timeout(700)
    if "click" in o:
        page.click(o["click"])
    if "keys" in o:
        page.keyboard.press(o["keys"])
    page.wait_for_timeout(int(o.get("wait", 250)))
    suffix = ("" if width == 1280 else f"-{width}") + ("-forced" if a.forced else "")
    file = os.path.join(a.out, f"{name}-{theme}{suffix}.png")
    page.screenshot(path=file, full_page=o.get("full") == "1")
    print(file)
    ctx.close()


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", default=os.environ.get("DAYTRACE_SHOTS", os.path.join(ROOT, "shots")))
    ap.add_argument("--only", default="", help="only routes whose name contains this text")
    ap.add_argument("--route", action="append", default=[], help="name=path[|options]; replaces the default list")
    ap.add_argument("--width", type=int, default=1280, help="width of routes without w=")
    ap.add_argument("--widths", default="", help="comma-separated widths; every route at each (overrides w=)")
    ap.add_argument("--forced", action="store_true", help="emulate forced-colors: active (Contrast themes)")
    ap.add_argument("--height", type=int, default=800)
    ap.add_argument("--themes", default="light,dark")
    ap.add_argument("--time", default="2026-10-16T17:42:00", help="fixed local time, or 'now'")
    ap.add_argument("--url", default="", help="an already running dev server (default: start one)")
    a = ap.parse_args()

    routes = ROUTES
    if a.route:
        routes = []
        for r in a.route:
            name, _, rest = r.partition("=")
            path, _, opts = rest.partition("|")
            routes.append((name, path, opts))
    routes = [r for r in routes if a.only in r[0]]
    os.makedirs(a.out, exist_ok=True)
    os.environ.setdefault("PLAYWRIGHT_BROWSERS_PATH", "/opt/pw-browsers")

    server = None
    base = a.url.rstrip("/")
    if not base:
        port = free_port()
        base = f"http://127.0.0.1:{port}"
        # Own process group, so stopping it also stops the node process behind the vite shim.
        server = subprocess.Popen(
            [os.path.join(ROOT, "node_modules", ".bin", "vite"), "--host", "127.0.0.1", "--port", str(port), "--strictPort"],
            cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, env={**os.environ, "BROWSER": "none"},
            start_new_session=True,
        )
        if not wait_http(base + "/"):
            stop(server)
            sys.exit("The Vite dev server did not start.")

    from playwright.sync_api import sync_playwright

    try:
        with sync_playwright() as p:
            browser = p.chromium.launch()
            for theme in a.themes.split(","):
                for name, path, opts in routes:
                    o = parse_opts(opts)
                    widths = [int(w) for w in a.widths.split(",")] if a.widths else [int(o.get("w", a.width))]
                    for width in widths:
                        shoot(browser, base, a, name, path, o, theme, width)
            browser.close()
    finally:
        if server:
            stop(server)


if __name__ == "__main__":
    main()
