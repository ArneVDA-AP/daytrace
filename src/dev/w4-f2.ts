/*
 * DEV ONLY: browser-preview answers for the wave 4 views work (src/dev/mock-ipc.ts merges them over
 * its own). SYNTHETIC data only.
 *
 * - Categories carry "Counts as" (the backend fills it on load and save, config.rs sanitized).
 * - journal_take_prompt: the morning goal prompt. Due once per page load when today has no plan,
 *   as with ?goals=prompt; ?goalprompt=off answers "not due".
 * - insights_save_image: Save as PNG on Weekly; ?image=fail refuses the write.
 * Example: http://localhost:1420/?goals=prompt#/timeline (click anywhere: the prompt opens).
 */
import type { Category, Settings } from "@/api";
import { countsAsOf } from "@/views/insights/format";

type Args = Record<string, unknown>;
type Handlers = Record<string, (a: Args) => unknown>;

function withCountsAs(cats: Category[]): Category[] {
  return cats.map((c) => ({ ...c, countsAs: countsAsOf(c) }));
}

export function applyF2Mock(base: Handlers, ctx: { q: URLSearchParams; settings: Settings; today: string }) {
  const s = ctx.settings;
  s.categories = withCountsAs(s.categories);
  const goals = new Set((ctx.q.get("goals") ?? "").split(",").filter(Boolean));
  let promptShown = ctx.q.get("goalprompt") === "off";

  const saveSettings = base.save_settings;
  const getSettings = base.get_settings;
  const handlers: Handlers = {
    save_settings: (a) => {
      saveSettings(a);
      s.categories = withCountsAs(s.categories);
      s.recapModel = s.recapModel.trim();
      return getSettings({});
    },
    journal_take_prompt: () => {
      if (!s.showGoalPrompt || promptShown) return null;
      const view = base.journal_get({ day: ctx.today }) as { prompt: boolean; plan: { saved: boolean } };
      if (!goals.has("prompt") || view.plan.saved) return null;
      promptShown = true;
      return view;
    },
    insights_save_image: (a) => {
      if (ctx.q.get("image") === "fail") throw `Access is denied. (os error 5) while writing ${String(a.path)}`;
      if (!String(a.path).toLowerCase().endsWith(".png")) throw "Choose a full path for a .png file.";
      return null;
    },
    "plugin:dialog|save": (a) => {
      const name = ((a.options ?? {}) as { defaultPath?: string }).defaultPath ?? "daytrace-export.md";
      return name.endsWith(".png") ? `C:\\Users\\Preview\\Pictures\\${name}` : "C:\\Users\\Preview\\Documents\\daytrace-export.md";
    },
  };
  Object.assign(base, handlers);
}
