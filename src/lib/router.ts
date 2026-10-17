import { useSyncExternalStore } from "react";

/*
 * Hash routing: #/timeline?day=2026-10-16&card=104, #/settings?section=storage.
 * WebView2 keeps history for hash changes, so the mouse Back button and Alt+Left/Right work,
 * citations and recap items can be plain links, and every state can be opened directly (and
 * screenshotted). Selecting inside a list replaces the history entry (navigate(..., {replace})).
 */

export type Section = "timeline" | "daily" | "weekly" | "journal" | "chat" | "settings";

export const SECTIONS: Section[] = ["timeline", "daily", "weekly", "journal", "chat", "settings"];

/** Sections that show one day (Weekly shows the week that contains it). They share `day`. */
export const DAY_SECTIONS: Section[] = ["timeline", "daily", "weekly", "journal"];

export interface Route {
  section: Section;
  params: URLSearchParams;
}

export type Params = Record<string, string | number | null | undefined>;

function parse(hash: string): Route {
  const raw = hash.replace(/^#\/?/, "");
  const q = raw.indexOf("?");
  const path = q === -1 ? raw : raw.slice(0, q);
  const query = q === -1 ? "" : raw.slice(q + 1);
  const section = (SECTIONS as string[]).includes(path) ? (path as Section) : "timeline";
  return { section, params: new URLSearchParams(query) };
}

let cached: { hash: string; route: Route } = { hash: "\0", route: parse("") };
function snapshot(): Route {
  if (cached.hash !== location.hash) cached = { hash: location.hash, route: parse(location.hash) };
  return cached.route;
}

function subscribe(cb: () => void) {
  addEventListener("hashchange", cb);
  return () => removeEventListener("hashchange", cb);
}

/** The current route; re-renders on every hash change. */
export function useRoute(): Route {
  return useSyncExternalStore(subscribe, snapshot);
}

/** The current route outside React. */
export function currentRoute(): Route {
  return snapshot();
}

/** An href for a section. Empty, null and undefined params are dropped. */
export function href(section: Section, params: Params = {}): string {
  const q = new URLSearchParams();
  for (const [k, v] of Object.entries(params)) if (v !== undefined && v !== null && v !== "") q.set(k, String(v));
  const s = q.toString();
  return `#/${section}${s ? `?${s}` : ""}`;
}

/** Goes to a section. `replace` swaps the current history entry instead of adding one. */
export function navigate(section: Section, params: Params = {}, opts: { replace?: boolean } = {}) {
  const h = href(section, params);
  if (h === location.hash) return;
  if (opts.replace) {
    history.replaceState(history.state, "", h);
    dispatchEvent(new HashChangeEvent("hashchange"));
  } else {
    location.hash = h;
  }
}

/** Changes some params of the current route (null removes one). Replaces history by default. */
export function setParams(patch: Params, opts: { replace?: boolean } = { replace: true }) {
  const r = snapshot();
  const next: Params = {};
  r.params.forEach((v, k) => (next[k] = v));
  for (const [k, v] of Object.entries(patch)) next[k] = v;
  navigate(r.section, next, opts);
}

const DAY_RE = /^\d{4}-\d{2}-\d{2}$/;

/** A valid `day` param, or null. */
export function dayParam(params: URLSearchParams): string | null {
  const d = params.get("day");
  return d && DAY_RE.test(d) ? d : null;
}

/** A numeric param (e.g. `card`), or null. */
export function numParam(params: URLSearchParams, key: string): number | null {
  const v = params.get(key);
  if (v === null || v === "") return null;
  const n = Number(v);
  return Number.isFinite(n) ? n : null;
}
