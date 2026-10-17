// Day rollover and idle reset (Dayflow's MainView.handleMinuteTickForDayChange and
// InactivityMonitor). Pure functions; the hook in platform-hooks.ts wires them to the window.

/** Dayflow's InactivityMonitor default threshold: 15 minutes. */
export const IDLE_RESET_MS = 15 * 60 * 1000;

/**
 * The day to show after the logical day changed (4 AM). Someone looking at "today" moves to the
 * new today; someone looking at an older day stays there.
 */
export function rolloverDay(viewed: string, previousToday: string | null, today: string): string {
  if (previousToday !== null && previousToday !== today && viewed === previousToday) return today;
  return viewed;
}

/**
 * Whether the view should go back to today and now: no interaction for `threshold` ms, and no
 * reset since then (Dayflow's checkIdle: a reset happens at most once per idle stretch).
 */
export function shouldIdleReset(now: number, lastInteraction: number, lastReset: number | null, threshold = IDLE_RESET_MS): boolean {
  if (now - lastInteraction < threshold) return false;
  if (lastReset !== null && lastReset >= lastInteraction) return false;
  return true;
}
