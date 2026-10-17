import { api, type Settings, type SettingsDto } from "@/api";
import { withLiveRecordingState } from "@/views/platform-api";
import { retryStale } from "./settings-retry";

/*
 * One queue for every settings write in the app (Settings pages, the rail's theme switch, the
 * command menu): writes run one after another, each as a patch on a fresh get_settings, so two
 * quick changes never overwrite each other and changes made elsewhere meanwhile (a pause from the
 * tray, the chat model) are kept. Recording on/off, the pause end and the setup flag still go
 * through withLiveRecordingState, as every settings save does. The queue lives outside React, so a
 * save started while leaving a view still finishes.
 *
 * Each save passes the revision of the settings it read. A change made outside the window between
 * the read and the save (the tray, an AI tool, the platform loop) makes the save fail as stale;
 * the patch is then applied again to a fresh read (settings-retry.ts), so that change is kept.
 */

export type Patch = Partial<Settings> | ((current: Settings) => Partial<Settings>);

let queue: Promise<unknown> = Promise.resolve();

/** Runs `job` after every settings write queued before it. */
export function afterSaves<T>(job: () => Promise<T>): Promise<T> {
  const run = queue.then(job, job);
  queue = run.catch(() => undefined);
  return run;
}

/** Saves a change, and an API key for the provider in the saved settings when one is given. */
export function savePatch(patch: Patch, apiKey?: string): Promise<SettingsDto> {
  return afterSaves(() =>
    retryStale(async () => {
      const dto = await api.getSettings();
      const change = typeof patch === "function" ? patch(dto.settings) : patch;
      return api.saveSettings(await withLiveRecordingState({ ...dto.settings, ...change }), apiKey || undefined, dto.revision);
    }),
  );
}

/** The current settings, read after the queued writes (for tests and model lists). */
export function savedSettings(): Promise<Settings> {
  return afterSaves(async () => (await api.getSettings()).settings);
}
