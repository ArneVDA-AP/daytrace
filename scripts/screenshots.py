#!/usr/bin/env python3
"""Screenshots of the browser preview (Vite dev server + the mocked IPC in src/dev), light and dark.

    npm run screenshots -- --out /path/to/folder                  # the default route list below
    npm run screenshots -- --out DIR --only timeline              # names containing "timeline"
    npm run screenshots -- --out DIR --route daily-empty='/?empty=1#/daily'
    npm run screenshots -- --out DIR --width 760 --height 800     # narrow window
    npm run screenshots -- --out DIR --url http://localhost:1420  # use a dev server you started

Writes <out>/<name>-light.png and <out>/<name>-dark.png. Uses Playwright's Chromium from
PLAYWRIGHT_BROWSERS_PATH (default /opt/pw-browsers); it never downloads browsers.

The clock is fixed (default Friday 16 October 2026, 5:42 PM local) so the synthetic data and the
"now" line look the same on every run; --time now uses the real clock. Segoe UI Variable is not on
Linux: the harness maps it to Inter if that is installed (the app itself never ships Inter).
Route options after a '|': w=760 (width), h=900 (height), keys=Control+k (press keys after load),
click=<css selector>, wait=800 (extra ms), full=1 (full page).
"""
import argparse
import os
import socket
import subprocess
import sys
import time
import urllib.request
from datetime import datetime

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DAY = "2026-10-16"
PREV = "2026-10-15"

# (name, path after the origin, options)
ROUTES = [
    ("timeline", f"/#/timeline?day={DAY}", ""),
    ("timeline-earlier-day", f"/#/timeline?day={PREV}", ""),
    ("timeline-no-data", "/#/timeline?day=2026-10-14", ""),
    ("timeline-empty-install", "/?empty=1#/timeline", ""),
    ("timeline-760", f"/#/timeline?day={DAY}", "w=760"),
    ("daily", f"/#/daily?day={DAY}", ""),
    ("weekly", f"/#/weekly?day={DAY}", ""),
    ("goals", f"/#/journal?day={DAY}", ""),
    ("chat", "/#/chat", ""),
    ("settings", "/#/settings", ""),
    ("onboarding", "/?onboarding=1#/timeline", ""),
    ("status-paused", f"/?status=paused#/timeline?day={DAY}", ""),
    ("status-paused-timed", f"/?status=paused-timed#/timeline?day={DAY}", ""),
    ("status-locked-writing", f"/?status=locked,writing#/timeline?day={DAY}", ""),
    ("status-away-batch", f"/?status=away,batch#/timeline?day={DAY}", ""),
    ("banner-nokey", f"/?status=nokey#/timeline?day={DAY}", ""),
    ("banner-offline", f"/?status=offline#/daily?day={DAY}", ""),
    ("banner-capture-error", f"/?status=capture-error#/timeline?day={DAY}", ""),
    ("banner-all-760", f"/?status=capture-error,offline#/timeline?day={DAY}", "w=760"),
    ("status-popover-760", f"/?status=offline#/timeline?day={DAY}", "w=760|click=[aria-label^='Status:']"),
    ("command-menu", f"/#/timeline?day={DAY}", "keys=Control+k|wait=600"),
    ("day-picker", f"/#/timeline?day={DAY}", "click=[aria-label='Go to a day']|wait=400"),
    ("pause-menu", f"/#/timeline?day={DAY}", "click=[aria-label='Pause recording for…']|wait=400"),
]

FONT_CSS = """
@font-face { font-family: "Segoe UI Variable Text"; src: local("Inter"), local("Inter Regular"); font-weight: 100 900; }
@font-face { font-family: "Segoe UI Variable Display"; src: local("Inter Display"), local("Inter"); font-weight: 100 900; }
"""


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


def parse_opts(s):
    out = {}
    for part in filter(None, s.split("|")):
        k, _, v = part.partition("=")
        out[k] = v
    return out


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--out", default=os.environ.get("DAYTRACE_SHOTS", os.path.join(ROOT, "shots")))
    ap.add_argument("--only", default="", help="only routes whose name contains this text")
    ap.add_argument("--route", action="append", default=[], help="name=path[|options]; replaces the default list")
    ap.add_argument("--width", type=int, default=1280)
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
        server = subprocess.Popen(
            ["npx", "vite", "--host", "127.0.0.1", "--port", str(port), "--strictPort"],
            cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.STDOUT, env={**os.environ, "BROWSER": "none"},
        )
        if not wait_http(base + "/"):
            server.terminate()
            sys.exit("The Vite dev server did not start.")

    from playwright.sync_api import sync_playwright

    try:
        with sync_playwright() as p:
            browser = p.chromium.launch()
            for theme in a.themes.split(","):
                for name, path, opts in routes:
                    o = parse_opts(opts)
                    ctx = browser.new_context(
                        viewport={"width": int(o.get("w", a.width)), "height": int(o.get("h", a.height))},
                        color_scheme=theme, reduced_motion="reduce", device_scale_factor=1,
                    )
                    page = ctx.new_page()
                    page.on("pageerror", lambda e, n=name: print(f"  page error in {n}: {e}", file=sys.stderr))
                    if a.time != "now":
                        page.clock.set_fixed_time(datetime.fromisoformat(a.time))
                    page.goto(base + path)
                    page.add_style_tag(content=FONT_CSS)
                    page.wait_for_selector("#content, .legacy", timeout=15000)
                    page.evaluate("document.fonts.ready")
                    page.wait_for_timeout(700)
                    if "click" in o:
                        page.click(o["click"])
                    if "keys" in o:
                        page.keyboard.press(o["keys"])
                    page.wait_for_timeout(int(o.get("wait", 250)))
                    page.mouse.move(0, 0) if "click" not in o else None
                    file = os.path.join(a.out, f"{name}-{theme}.png")
                    page.screenshot(path=file, full_page=o.get("full") == "1")
                    print(file)
                    ctx.close()
            browser.close()
    finally:
        if server:
            server.terminate()


if __name__ == "__main__":
    main()
