import * as React from "react";
import { timelineApi } from "./api";

/*
 * Screenshot images for the timelapse, the contact strip and the viewer. They come over IPC as
 * data URLs (CSP img-src 'self' data:), so they are cached here: thumbnails (360 px high) for the
 * player, a few full-size images for the viewer. null means the file is gone (retention, storage
 * limit, or a private app was in front).
 */

const THUMB_LIMIT = 400;
const FULL_LIMIT = 6;
const PARALLEL = 3;

const thumbs = new Map<number, string | null>();
const inflight = new Map<number, Promise<string | null>>();
const full = new Map<number, string | null>();
const listeners = new Set<() => void>();

function remember(cache: Map<number, string | null>, id: number, src: string | null, limit: number) {
  cache.set(id, src);
  while (cache.size > limit) {
    const oldest = cache.keys().next().value;
    if (oldest === undefined) break;
    cache.delete(oldest);
  }
}

export function cachedThumb(id: number): string | null | undefined {
  return thumbs.get(id);
}

export function hasThumb(id: number): boolean {
  return thumbs.has(id);
}

function loadThumb(id: number): Promise<string | null> {
  if (thumbs.has(id)) return Promise.resolve(thumbs.get(id) ?? null);
  const running = inflight.get(id);
  if (running) return running;
  const p = timelineApi
    .screenshot(id, true)
    .catch(() => null)
    .then((src) => {
      inflight.delete(id);
      remember(thumbs, id, src, THUMB_LIMIT);
      listeners.forEach((l) => l());
      return src;
    });
  inflight.set(id, p);
  return p;
}

let queue: number[] = [];
let active = 0;

function pump() {
  while (active < PARALLEL && queue.length) {
    const id = queue.shift()!;
    if (thumbs.has(id) || inflight.has(id)) continue;
    active += 1;
    void loadThumb(id).finally(() => {
      active -= 1;
      pump();
    });
  }
}

/** Loads these thumbnails first (nearest first), a few at a time. */
export function wantThumbs(ids: number[]) {
  const fresh = ids.filter((id) => !thumbs.has(id));
  queue = [...fresh, ...queue.filter((id) => !fresh.includes(id))];
  pump();
}

/** Re-renders the caller when any thumbnail arrives. */
export function useThumbTick(): number {
  const [tick, setTick] = React.useState(0);
  React.useEffect(() => {
    const l = () => setTick((t) => t + 1);
    listeners.add(l);
    return () => {
      listeners.delete(l);
    };
  }, []);
  return tick;
}

/** A full-size screenshot for the viewer (cached for the last few). */
export async function fullImage(id: number): Promise<string | null> {
  if (full.has(id)) return full.get(id) ?? null;
  const src = await timelineApi.screenshot(id, false).catch(() => null);
  remember(full, id, src, FULL_LIMIT);
  return src;
}
