import * as React from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type Category, type SettingsDto, type Status } from "@/api";
import { makeColorFor } from "@/lib/categories";
import { applyAppearance, useAppearanceSync, type Appearance } from "@/lib/theme";
import { errorText } from "@/lib/utils";
import { platformApi, withLiveRecordingState } from "@/views/platform-api";
import { describeAnalysis, describeRecorder, problems, type AnalysisView, type Problems, type RecorderView } from "@/app/status";

/*
 * App-wide state: settings (get_settings, reloaded on the backend's "settings-changed" event) and
 * status (get_status every 5 s while the window is visible). Views read it with useAppState();
 * they never poll status themselves.
 */

const POLL_MS = 5000;

export interface AppState {
  settings: SettingsDto;
  status: Status;
  today: string;
  categories: Category[];
  /** CSS colour for a card's category name (theme-aware for the recommended defaults). */
  colorFor: (category: string) => string;
  recorder: RecorderView;
  analysis: AnalysisView;
  problems: Problems;
  /** Reloads settings and status now. */
  reload: () => Promise<void>;
  reloadStatus: () => Promise<void>;
  /** Puts a settings object returned by save_settings into the app state. */
  setSettings: (dto: SettingsDto) => void;
  /** Pause for a preset ("15", "tomorrow", "indefinite"); throws the backend's message. */
  pause: (choice: string) => Promise<void>;
  resume: () => Promise<void>;
  /** Saves Settings > Appearance (and applies it at once). */
  saveAppearance: (a: Appearance) => Promise<void>;
}

const Ctx = React.createContext<AppState | null>(null);

export function useAppState(): AppState {
  const v = React.useContext(Ctx);
  if (!v) throw new Error("useAppState() outside <AppStateProvider>");
  return v;
}

function same<T>(a: T, b: T): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

type Load = { kind: "loading" } | { kind: "error"; message: string } | { kind: "ready"; settings: SettingsDto; status: Status };

export function AppStateProvider({ children, fallback }: { children: React.ReactNode; fallback: (s: { error: string | null; retry: () => void }) => React.ReactNode }) {
  const [load, setLoad] = React.useState<Load>({ kind: "loading" });
  const latest = React.useRef(load);
  latest.current = load;

  const fetchAll = React.useCallback(async () => {
    try {
      const [settings, status] = await Promise.all([api.getSettings(), api.getStatus()]);
      // Apply the theme before the first render of the shell, so it paints in the right colours.
      if (latest.current.kind !== "ready") applyAppearance(settings.settings.appearance);
      setLoad((cur) => {
        if (cur.kind === "ready" && same(cur.settings, settings) && same(cur.status, status)) return cur;
        return { kind: "ready", settings, status };
      });
    } catch (e) {
      if (latest.current.kind !== "ready") setLoad({ kind: "error", message: errorText(e) });
    }
  }, []);

  const reloadStatus = React.useCallback(async () => {
    try {
      const status = await api.getStatus();
      // Only re-render when something changed, so a poll never moves anything on screen.
      setLoad((cur) => (cur.kind === "ready" && !same(cur.status, status) ? { ...cur, status } : cur));
    } catch {
      // Keep the last known status; the next poll tries again.
    }
  }, []);

  React.useEffect(() => {
    void fetchAll();
    const tick = () => {
      if (document.visibilityState === "visible") void reloadStatus();
    };
    const t = setInterval(tick, POLL_MS);
    document.addEventListener("visibilitychange", tick);
    const un = listen("settings-changed", () => void fetchAll());
    return () => {
      clearInterval(t);
      document.removeEventListener("visibilitychange", tick);
      void un.then((f) => f());
    };
  }, [fetchAll, reloadStatus]);

  const ready = load.kind === "ready" ? load : null;
  useAppearanceSync(ready?.settings.settings.appearance);

  const categories = React.useMemo(() => ready?.settings.settings.categories ?? [], [ready?.settings]);
  const colorFor = React.useMemo(() => makeColorFor(categories), [categories]);

  const value = React.useMemo<AppState | null>(() => {
    if (!ready) return null;
    const { settings, status } = ready;
    return {
      settings,
      status,
      today: status.today,
      categories,
      colorFor,
      recorder: describeRecorder(status, settings),
      analysis: describeAnalysis(status, settings),
      problems: problems(status, settings),
      reload: fetchAll,
      reloadStatus,
      setSettings: (dto) => setLoad((cur) => (cur.kind === "ready" ? { ...cur, settings: dto } : cur)),
      pause: async (choice) => {
        await platformApi.pause(choice);
        await fetchAll();
      },
      resume: async () => {
        await platformApi.resume();
        await fetchAll();
      },
      saveAppearance: async (a) => {
        applyAppearance(a);
        // Start from the backend's copy so a pause made from the tray meanwhile is kept.
        const fresh = (await api.getSettings()).settings;
        const dto = await api.saveSettings(await withLiveRecordingState({ ...fresh, appearance: a }));
        setLoad((cur) => (cur.kind === "ready" ? { ...cur, settings: dto } : cur));
      },
    };
  }, [ready, categories, colorFor, fetchAll, reloadStatus]);

  if (!value) return <>{fallback({ error: load.kind === "error" ? load.message : null, retry: () => void fetchAll() })}</>;
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}
