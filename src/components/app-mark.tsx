import * as React from "react";
import { invoke } from "@tauri-apps/api/core";
import { useAppSettings } from "@/app/state";
import { cn } from "@/lib/utils";

/*
 * The icon of an app or site, next to its name (Dayflow's FaviconImageView): the program's own
 * icon from this PC, a site's favicon when Settings > Appearance > "Look up site icons" is on,
 * otherwise a letter badge. Always decorative: the name is written next to it, so the image has
 * alt="" and the badge is aria-hidden. With "Show app and site icons" off it renders nothing.
 *
 * Names are collected for 30 ms and asked for in one icons_lookup call (features/icons.rs); the
 * answers are kept for the session. While the first answer is on its way an empty box of the
 * same size holds the place, so nothing moves when the icon arrives.
 */

export interface IconDto {
  /** `data:image/png;base64,…`, 32×32. */
  src: string;
  /** A dark mark on a transparent background: drawn light in dark mode. */
  tint: boolean;
}

interface IconLookup {
  icons: Record<string, IconDto | null>;
  /** Names whose icon is being made or fetched now. */
  pending: string[];
}

interface Entry {
  icon: IconDto | null;
  at: number;
}

/** A name without an icon is asked for again after this long (a program seen for the first time). */
const MISS_MS = 10 * 60_000;
/** Asks again for names the backend is still working on (local icon, site lookup). */
const PENDING_RETRY_MS = [1500, 4000, 10_000];
const BATCH = 100;

class IconStore {
  private entries = new Map<string, Entry>();
  private queued = new Set<string>();
  private inflight = new Set<string>();
  private retries = new Map<string, number>();
  private listeners = new Set<() => void>();
  private timer: ReturnType<typeof setTimeout> | null = null;
  private generation = "";

  subscribe = (l: () => void) => {
    this.listeners.add(l);
    return () => {
      this.listeners.delete(l);
    };
  };

  get(name: string): Entry | undefined {
    return this.entries.get(name);
  }

  /** Settings that change the answers (site lookup, private apps): start over when they change. */
  setGeneration(g: string) {
    if (g === this.generation) return;
    const first = this.generation === "";
    this.generation = g;
    if (first) return;
    this.entries.clear();
    this.retries.clear();
    this.notify();
  }

  want(name: string) {
    const e = this.entries.get(name);
    if (e && (e.icon || Date.now() - e.at < MISS_MS)) return;
    if (this.inflight.has(name)) return;
    this.queued.add(name);
    this.schedule(30);
  }

  private schedule(ms: number) {
    if (this.timer == null) this.timer = setTimeout(() => void this.flush(), ms);
  }

  private notify() {
    this.listeners.forEach((l) => l());
  }

  private async flush() {
    this.timer = null;
    const names = [...this.queued].slice(0, BATCH);
    for (const n of names) {
      this.queued.delete(n);
      this.inflight.add(n);
    }
    if (this.queued.size) this.schedule(30);
    const gen = this.generation;
    let answer: IconLookup | null = null;
    try {
      answer = await invoke<IconLookup>("icons_lookup", { names });
    } catch {
      answer = null; // letters; asked again after MISS_MS
    }
    for (const n of names) this.inflight.delete(n);
    if (gen !== this.generation) return;
    const now = Date.now();
    for (const n of names) this.entries.set(n, { icon: answer?.icons[n] ?? null, at: now });
    for (const n of answer?.pending ?? []) {
      const tries = this.retries.get(n) ?? 0;
      if (tries >= PENDING_RETRY_MS.length) continue;
      this.retries.set(n, tries + 1);
      setTimeout(() => {
        if (gen !== this.generation || this.entries.get(n)?.icon) return;
        this.queued.add(n);
        this.schedule(0);
      }, PENDING_RETRY_MS[tries]);
    }
    this.notify();
  }
}

const store = new IconStore();

/** The first letter or digit of a name, for the badge ("github.com" → "G"). */
function initial(name: string): string {
  for (const ch of name) if (/[\p{L}\p{N}]/u.test(ch)) return ch.toLocaleUpperCase();
  return "?";
}

/** 16 px icon or letter badge for an app or site name; inline, so it sits in a line of text. */
export function AppMark({ name, className }: { name: string | null | undefined; className?: string }) {
  const s = useAppSettings().settings.settings;
  const show = s.showIcons ?? true;
  const label = name?.trim() ?? "";
  const generation = `${s.lookupSiteIcons ? 1 : 0}|${s.blockedApps.join("\n")}`;
  const entry = React.useSyncExternalStore(store.subscribe, () => (label ? store.get(label) : undefined));

  React.useEffect(() => store.setGeneration(generation), [generation]);
  React.useEffect(() => {
    if (show && label) store.want(label);
  }, [show, label, entry, generation]);

  if (!show || !label) return null;
  const box = "inline-block size-4 shrink-0 rounded-[3px] align-middle";
  if (!entry) return <span aria-hidden="true" data-slot="app-mark" className={cn(box, className)} />;
  if (entry.icon)
    return (
      <img
        src={entry.icon.src}
        alt=""
        width={16}
        height={16}
        draggable={false}
        data-slot="app-mark"
        className={cn(box, "object-contain", entry.icon.tint && "dark:invert", className)}
      />
    );
  return (
    <span
      aria-hidden="true"
      data-slot="app-mark"
      className={cn(box, "inline-grid place-items-center border border-border-strong bg-surface-2 text-[10px] leading-none font-semibold text-muted-foreground", className)}
    >
      {initial(label)}
    </span>
  );
}
