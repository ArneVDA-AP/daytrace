import { useEffect, useSyncExternalStore } from "react";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";

/*
 * Light/dark comes from Settings > Appearance (settings.appearance, stored by the backend), never
 * from localStorage. The backend creates the window with the same theme (tray.rs show_main), so
 * WebView2's prefers-color-scheme already matches before any script runs: no flash.
 *
 * applyAppearance() mirrors the choice onto <html data-theme> (tokens.css reads it) and onto the
 * native window: title bar theme (setTheme) and the window/webview background colour, which shows
 * while the window resizes or before the page paints.
 */

export type Appearance = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";

/** --background of each theme as hex (tokens.css), for the native window. Keep in sync with tray.rs. */
export const WINDOW_BACKGROUND: Record<ResolvedTheme, string> = { light: "#f4f6f8", dark: "#0f1113" };

const DARK_QUERY = "(prefers-color-scheme: dark)";
let current: Appearance = "system";
const listeners = new Set<() => void>();

function systemDark(): boolean {
  return typeof matchMedia === "function" && matchMedia(DARK_QUERY).matches;
}

export function resolveTheme(a: Appearance): ResolvedTheme {
  if (a === "system") return systemDark() ? "dark" : "light";
  return a;
}

/** True inside the Tauri webview (or the browser preview's mocked IPC). */
function hasIpc(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

async function syncWindow(a: Appearance) {
  if (!hasIpc()) return;
  try {
    const w = getCurrentWebviewWindow();
    await w.setTheme(a === "system" ? null : a);
    await w.setBackgroundColor(WINDOW_BACKGROUND[resolveTheme(a)]);
  } catch {
    // Older capability sets or a closed window: the CSS theme still applies.
  }
}

/** Applies an appearance to the page and the native window. Safe to call repeatedly. */
export function applyAppearance(a: Appearance) {
  const root = document.documentElement;
  if (a === "system") root.removeAttribute("data-theme");
  else root.setAttribute("data-theme", a);
  const changed = a !== current;
  current = a;
  void syncWindow(a);
  if (changed) listeners.forEach((l) => l());
}

/** Keeps the page and the window in line with `appearance` (from settings) while mounted. */
export function useAppearanceSync(appearance: Appearance | undefined) {
  useEffect(() => {
    if (!appearance) return;
    applyAppearance(appearance);
    if (appearance !== "system") return;
    // Following Windows: the window background must follow a theme switch in Windows too.
    const mq = matchMedia(DARK_QUERY);
    const on = () => void syncWindow("system");
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, [appearance]);
}

function subscribe(cb: () => void) {
  listeners.add(cb);
  const mq = matchMedia(DARK_QUERY);
  mq.addEventListener("change", cb);
  return () => {
    listeners.delete(cb);
    mq.removeEventListener("change", cb);
  };
}

/** The theme on screen right now, for the rare code that cannot use CSS variables. */
export function useResolvedTheme(): ResolvedTheme {
  return useSyncExternalStore(subscribe, () => resolveTheme(current));
}
