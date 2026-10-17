/*
 * DEV ONLY: icons_lookup for the browser preview (app and site icons, w5-r3). The icons are
 * invented shapes (colored tiles with a plain white figure), not the real programs' or sites'
 * marks. Variants, as a query parameter before the hash:
 *   ?icons=off       Settings > Appearance > "Show app and site icons" off: no marks at all
 *   ?icons=sites     "Look up site icons" on: the public sites get icons too
 *   ?icons=letters   no icon found for anything: letter badges everywhere
 *   ?icons=slow      the first answer for every app is "pending"; the icons arrive on the re-ask
 * Default: icons for the programs in the fixtures, letters for sites (the lookup is off).
 */
import type { Settings } from "@/api";

type Args = Record<string, unknown>;
interface MockIcon {
  src: string;
  tint: boolean;
}

function tile(color: string, figure: string): MockIcon {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32"><rect x="2" y="2" width="28" height="28" rx="7" fill="${color}"/>${figure}</svg>`;
  return { src: `data:image/svg+xml;base64,${btoa(svg)}`, tint: false };
}

/** A black figure on transparent, like many site marks (drawn light in dark mode). */
function silhouette(figure: string): MockIcon {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" fill="#111">${figure}</svg>`;
  return { src: `data:image/svg+xml;base64,${btoa(svg)}`, tint: true };
}

const W = 'fill="#fff"';
const APPS: Record<string, MockIcon> = {
  visualstudiocode: tile("#2b6fd6", `<path ${W} d="M9 16l6-6v3l-3 3 3 3v3zM23 16l-6-6v3l3 3-3 3v3z"/>`),
  microsoftedge: tile("#178a7e", `<circle ${W} cx="16" cy="16" r="7"/><circle fill="#178a7e" cx="18" cy="17" r="3.5"/>`),
  microsoftteams: tile("#5b5fc7", `<rect ${W} x="9" y="10" width="14" height="4" rx="1"/><rect ${W} x="14" y="10" width="4" height="13" rx="1"/>`),
  windowsterminal: tile("#2d2f36", `<path ${W} d="M8 11l5 5-5 5-1.5-1.5L10 16l-3.5-3.5zM14 20h10v2H14z"/>`),
  excel: tile("#1f7a45", `<path ${W} d="M9 9h5l2 4 2-4h5l-4.5 7 4.5 7h-5l-2-4-2 4H9l4.5-7z"/>`),
  outlook: tile("#1565c0", `<rect ${W} x="8" y="10" width="16" height="12" rx="2"/><path fill="#1565c0" d="M8 11l8 6 8-6v2l-8 6-8-6z"/>`),
  notion: tile("#e9e6df", `<path fill="#222" d="M10 9h4l5 9V9h3v14h-4l-5-9v9h-3z"/>`),
  linear: tile("#5e6ad2", `<path ${W} d="M8 18l6 6c-3-.5-5.5-3-6-6zm0-3.5L17.5 24H20L8 12zM11 9.5L22.5 21c.6-.8 1-1.6 1.3-2.5L13.5 8.2c-.9.3-1.7.7-2.5 1.3zM16 8a8 8 0 018 8z"/>`),
  obsidian: tile("#7c4dc4", `<path ${W} d="M16 7l7 6-3 12h-8l-3-12z"/>`),
  windowsperformanceanalyzer: tile("#b3471b", `<path ${W} d="M8 22h16v2H8zM9 20v-5h3v5zM14 20V10h3v10zM19 20v-7h3v7z"/>`),
};

const SITES: Record<string, MockIcon> = {
  "github.com": silhouette(`<circle cx="16" cy="16" r="12"/><circle fill="#fff" cx="16" cy="14" r="5"/>`),
  "docs.rs": tile("#d7681c", `<rect ${W} x="10" y="9" width="12" height="15" rx="1.5"/><path fill="#d7681c" d="M12 12h8v1.5h-8zM12 15h8v1.5h-8zM12 18h5v1.5h-5z"/>`),
  "developer.mozilla.org": silhouette(`<path d="M6 25L14 7h3L9 25zM14 25l8-18h3l-8 18z"/>`),
  "learn.microsoft.com": tile("#3a6fb0", `<path ${W} d="M8 12l8-4 8 4-8 4zM11 15v5l5 2.5 5-2.5v-5l-5 2.5z"/>`),
  "reddit.com": tile("#e5532d", `<circle ${W} cx="16" cy="18" r="7"/><circle fill="#e5532d" cx="13.5" cy="17.5" r="1.3"/><circle fill="#e5532d" cx="18.5" cy="17.5" r="1.3"/>`),
  "youtube.com": tile("#d93025", `<path ${W} d="M13 11l8 5-8 5z"/>`),
};

function norm(s: string) {
  return s.toLowerCase().replace(/[^\p{L}\p{N}]/gu, "");
}

/** Public-looking host names, like features/icons.rs site_host (".example" is reserved). */
function siteHost(name: string): string | null {
  const host = name.trim().toLowerCase().replace(/^https?:\/\//, "").split(/[/?#]/)[0];
  if (!/^[a-z0-9-]+(\.[a-z0-9-]+)+$/.test(host)) return null;
  const tld = host.split(".").pop() ?? "";
  return /^[a-z]{2,}$/.test(tld) && !["example", "local", "test", "invalid", "localhost", "lan"].includes(tld) ? host : null;
}

export function applyIconsMock(handlers: Record<string, (a: Args) => unknown>, ctx: { q: URLSearchParams; settings: Settings }) {
  const mode = ctx.q.get("icons") ?? "";
  if (mode === "off") ctx.settings.showIcons = false;
  if (mode === "sites") ctx.settings.lookupSiteIcons = true;
  const asked = new Set<string>();
  handlers.icons_lookup = (a) => {
    const names = Array.isArray(a.names) ? (a.names as string[]) : [];
    const icons: Record<string, MockIcon | null> = {};
    const pending: string[] = [];
    for (const name of names) {
      icons[name] = null;
      if (!ctx.settings.showIcons || mode === "letters") continue;
      if (ctx.settings.blockedApps.some((b) => norm(name).includes(norm(b)) && norm(b))) continue;
      const host = siteHost(name);
      if (host) {
        if (ctx.settings.lookupSiteIcons) icons[name] = SITES[host] ?? null;
        continue;
      }
      const icon = APPS[norm(name)] ?? null;
      if (mode === "slow" && icon && !asked.has(name)) {
        asked.add(name);
        pending.push(name);
        continue;
      }
      icons[name] = icon;
    }
    return { icons, pending };
  };
}
