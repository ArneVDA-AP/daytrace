import { useSyncExternalStore } from "react";

/*
 * Shortcuts match the physical key (KeyboardEvent.code), so they work on every layout. On a
 * Belgian AZERTY keyboard Ctrl+1 gives e.key "&" and "[" needs AltGr (which Windows reports as
 * Ctrl+Alt), but e.code is "Digit1" and "BracketLeft" on every layout. Labels for punctuation keys
 * come from the keyboard layout map when the WebView offers it, so the "[" key reads "^" on
 * AZERTY; letters and digits keep their usual names.
 */

export interface Mods {
  ctrl?: boolean;
  shift?: boolean;
  alt?: boolean;
}

type KeyLike = Pick<KeyboardEvent, "code" | "ctrlKey" | "shiftKey" | "altKey" | "metaKey">;

/** The event has exactly these modifiers (Meta never counts as a match). */
export function modsAre(e: KeyLike, m: Mods = {}): boolean {
  return e.ctrlKey === !!m.ctrl && e.shiftKey === !!m.shift && e.altKey === !!m.alt && !e.metaKey;
}

/** Physical key `code` with exactly the modifiers `m`. */
export function isKey(e: KeyLike, code: string, m: Mods = {}): boolean {
  return e.code === code && modsAre(e, m);
}

/** The digit row or the number pad: 1 for "Digit1" and "Numpad1", else null. */
export function digitOf(e: Pick<KeyboardEvent, "code">): number | null {
  const m = /^(?:Digit|Numpad)(\d)$/.exec(e.code);
  return m ? Number(m[1]) : null;
}

/** US names of the punctuation codes used by shortcuts, the fallback when there is no layout map. */
const US: Record<string, string> = { BracketLeft: "[", BracketRight: "]", Comma: ",", Period: ".", Slash: "/" };

let layout: Map<string, string> | null = null;
let requested = false;
const listeners = new Set<() => void>();

function loadLayout() {
  if (requested) return;
  requested = true;
  const kb = (navigator as Navigator & { keyboard?: { getLayoutMap?: () => Promise<Map<string, string>> } }).keyboard;
  kb?.getLayoutMap?.()
    .then((m) => {
      layout = new Map(m);
      listeners.forEach((l) => l());
    })
    .catch(() => undefined);
}

/** What the key with this code prints on the current layout ("[" on US, "^" on Belgian AZERTY). */
export function keyLabel(code: string): string {
  const fallback = US[code] ?? code.replace(/^(Key|Digit)/, "");
  if (!(code in US)) return fallback;
  const v = layout?.get(code);
  return v && v.trim() ? v : fallback;
}

/** keyLabel() that re-renders once the layout map has loaded. */
export function useKeyLabel(code: string): string {
  return useSyncExternalStore(
    (cb) => {
      loadLayout();
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    () => keyLabel(code),
  );
}

/** aria-keyshortcuts value for a code ("Control+1", "["). */
export function ariaKey(code: string, m: Mods = {}): string {
  const k = code.startsWith("Digit") ? code.slice(5) : code.startsWith("Key") ? code.slice(3) : (US[code] ?? code);
  return [m.ctrl && "Control", m.alt && "Alt", m.shift && "Shift", k].filter(Boolean).join("+");
}
