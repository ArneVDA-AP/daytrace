/*
 * Settings revisions (commands.rs save_settings). Every saved settings change counts up a
 * revision; a save that passes the revision it was made on is refused when the settings changed
 * since (a pause from the tray, a category an AI tool added, a timed pause that ended), with this
 * error. The save queue (settings-writes.ts) then reads the settings again and applies its
 * change to them, a few times at most.
 */

/** What save_settings gives back for a save made on settings that changed since. */
export interface StaleSettings {
  code: "settings_changed";
  message: string;
  revision: number;
}

export function isStaleSettings(e: unknown): e is StaleSettings {
  return typeof e === "object" && e !== null && (e as { code?: unknown }).code === "settings_changed";
}

/** How many times a save is made before a stale-settings error is passed on. */
export const SAVE_TRIES = 3;

/**
 * Runs `attempt` (read the settings, apply the change, save with the revision read) again while
 * it fails because the settings changed meanwhile, `tries` times at most. Other errors are passed
 * on at once; the last stale-settings error becomes an Error with its message.
 */
export async function retryStale<T>(attempt: () => Promise<T>, tries = SAVE_TRIES): Promise<T> {
  for (let i = 1; ; i++) {
    try {
      return await attempt();
    } catch (e) {
      if (!isStaleSettings(e)) throw e;
      if (i >= tries) throw new Error(e.message);
    }
  }
}
