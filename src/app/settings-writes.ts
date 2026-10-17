import { api, type Settings, type SettingsDto } from "@/api";
import { withLiveRecordingState } from "@/views/platform-api";

/*
 * One queue for every settings write in the app (Settings pages, the rail's theme switch, the
 * command menu): writes run one after another, each as a patch on a fresh get_settings, so two
 * quick changes never overwrite each other and changes made elsewhere meanwhile (a pause from the
 * tray, the chat model) are kept. Recording on/off, the pause end and the setup flag still go
 * through withLiveRecordingState, as every settings save does. The queue lives outside React, so a
 * save started while leaving a view still finishes.
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
  return afterSaves(async () => {
    const fresh = (await api.getSettings()).settings;
    const change = typeof patch === "function" ? patch(fresh) : patch;
    return api.saveSettings(await withLiveRecordingState({ ...fresh, ...change }), apiKey || undefined);
  });
}

/** The current settings, read after the queued writes (for tests and model lists). */
export function savedSettings(): Promise<Settings> {
  return afterSaves(async () => (await api.getSettings()).settings);
}
