import * as React from "react";
import { listen } from "@tauri-apps/api/event";
import { api, type Category, type SettingsDto, type Status } from "@/api";
import { makeColorFor } from "@/lib/categories";
import { applyAppearance, useAppearanceSync, type Appearance } from "@/lib/theme";
import { errorText } from "@/lib/utils";
import { platformApi } from "@/views/platform-api";
import { savePatch } from "@/app/settings-writes";
import { describeAnalysis, describeRecorder, problems, type AnalysisView, type Problems, type RecorderView } from "@/app/status";

/*
 * App-wide state: settings (get_settings, reloaded on the backend's "settings-changed" event) and
 * status (get_status every 5 s while the window is visible). Views never poll status themselves.
 *
 * A status change re-renders only what reads the status (FE5):
 *   - useAppSettings(): settings, categories, colors and the actions (a context);
 *   - useAppStatus() / useAppStatusSelect(sel): the status and its wording, from a store outside
 *     React's context, so a component re-renders only when what it selected changed;
 *   - useAppState(): both, tracked: it re-renders on a status change only when the component read
 *     a status field (status, today, recorder, analysis, problems) while rendering.
 * A poll that only moves lastCaptureAt within the same minute changes nothing on screen ("last at
 * 5:41 PM"), so it is not applied at all.
 */

const POLL_MS = 5000;

export interface AppSettingsState {
  settings: SettingsDto;
  categories: Category[];
  /** CSS color for a card's category name (theme-aware for the recommended defaults). */
  colorFor: (category: string) => string;
  /** Reloads settings and status now. */
  reload: () => Promise<void>;
  reloadStatus: () => Promise<void>;
  /** Puts a settings object returned by save_settings into the app state (newest result wins). */
  setSettings: (dto: SettingsDto) => void;
  /** Pause for a preset ("15", "tomorrow", "indefinite"); throws the backend's message. */
  pause: (choice: string) => Promise<void>;
  resume: () => Promise<void>;
  /** Saves Settings > Appearance (and applies it at once). */
  saveAppearance: (a: Appearance) => Promise<void>;
}

export interface AppStatusState {
  status: Status;
  today: string;
  recorder: RecorderView;
  analysis: AnalysisView;
  problems: Problems;
}

export type AppState = AppSettingsState & AppStatusState;

const STATUS_KEYS: (keyof AppStatusState)[] = ["status", "today", "recorder", "analysis", "problems"];

/** The latest status, with listeners (useSyncExternalStore). */
interface StatusStore {
  get: () => AppStatusState;
  set: (v: AppStatusState) => void;
  subscribe: (l: () => void) => () => void;
}

function createStatusStore(initial: AppStatusState): StatusStore {
  let value = initial;
  const listeners = new Set<() => void>();
  return {
    get: () => value,
    set: (v) => {
      if (v === value) return;
      value = v;
      listeners.forEach((l) => l());
    },
    subscribe: (l) => {
      listeners.add(l);
      return () => listeners.delete(l);
    },
  };
}

const SettingsCtx = React.createContext<AppSettingsState | null>(null);
const StatusStoreCtx = React.createContext<StatusStore | null>(null);

/** Settings, categories, colors and the actions; does not re-render on status polls. */
export function useAppSettings(): AppSettingsState {
  const v = React.useContext(SettingsCtx);
  if (!v) throw new Error("useAppSettings() outside <AppStateProvider>");
  return v;
}

function useStatusStore(): StatusStore {
  const store = React.useContext(StatusStoreCtx);
  if (!store) throw new Error("app status read outside <AppStateProvider>");
  return store;
}

/** Status and its wording (recorder, analysis, problems, today); re-renders on every change. */
export function useAppStatus(): AppStatusState {
  const store = useStatusStore();
  return React.useSyncExternalStore(store.subscribe, store.get);
}

/**
 * One value derived from the status; re-renders only when it changes (`equal`, default
 * Object.is). Keep the selector cheap and return primitives or stable objects.
 */
export function useAppStatusSelect<T>(select: (s: AppStatusState) => T, equal: (a: T, b: T) => boolean = Object.is): T {
  const store = useStatusStore();
  const last = React.useRef<{ v: T } | null>(null);
  const selectRef = React.useRef(select);
  selectRef.current = select;
  const getSnapshot = React.useCallback(() => {
    const next = selectRef.current(store.get());
    if (last.current && equal(last.current.v, next)) return last.current.v;
    last.current = { v: next };
    return next;
    // `equal` is expected to be stable (a module-level function).
  }, [store]); // eslint-disable-line react-hooks/exhaustive-deps
  return React.useSyncExternalStore(store.subscribe, getSnapshot);
}

/**
 * Settings, status and actions in one object. Status fields are read through getters, so the
 * component re-renders on a status change only when it read a status field while rendering;
 * event handlers that read one get the latest value.
 */
export function useAppState(): AppState {
  const settings = useAppSettings();
  const store = useStatusStore();
  const used = React.useRef(new Set<keyof AppStatusState>());
  const seen = React.useRef<AppStatusState>(store.get());
  const getSnapshot = React.useCallback(() => {
    const next = store.get();
    for (const k of used.current) {
      if (!Object.is(seen.current[k], next[k])) {
        seen.current = next;
        break;
      }
    }
    return seen.current;
  }, [store]);
  React.useSyncExternalStore(store.subscribe, getSnapshot);
  return React.useMemo(() => {
    const out = { ...settings } as AppState;
    for (const k of STATUS_KEYS) {
      Object.defineProperty(out, k, {
        enumerable: true,
        get: () => {
          used.current.add(k);
          return store.get()[k];
        },
      });
    }
    return out;
  }, [settings, store]);
}

function same<T>(a: T, b: T): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** Status equality as seen on screen: lastCaptureAt counts to the minute. */
export function sameStatus(a: Status, b: Status): boolean {
  const minute = (t: number | null) => (t === null ? null : Math.floor(t / 60));
  return minute(a.lastCaptureAt) === minute(b.lastCaptureAt) && same({ ...a, lastCaptureAt: 0 }, { ...b, lastCaptureAt: 0 });
}

type Load = { kind: "loading" } | { kind: "error"; message: string } | { kind: "ready"; settings: SettingsDto; status: Status };

export function AppStateProvider({ children, fallback }: { children: React.ReactNode; fallback: (s: { error: string | null; retry: () => void }) => React.ReactNode }) {
  const [load, setLoad] = React.useState<Load>({ kind: "loading" });
  const latest = React.useRef(load);
  latest.current = load;

  // Responses are applied in order: every read takes a ticket when it starts, every save when its
  // result arrives, and a result older than the last one applied is dropped. So a refetch that
  // started before a save landed (settings-changed, a poll) cannot put the old value back.
  const order = React.useRef({ next: 0, settings: 0, status: 0 });
  const ticket = () => ++order.current.next;
  const claim = (part: "settings" | "status", t: number) => {
    if (t <= order.current[part]) return false;
    order.current[part] = t;
    return true;
  };

  const applySettings = React.useCallback((dto: SettingsDto) => {
    if (!claim("settings", ticket())) return;
    setLoad((cur) => (cur.kind === "ready" && !same(cur.settings, dto) ? { ...cur, settings: dto } : cur));
  }, []);

  const fetchAll = React.useCallback(async () => {
    const t = ticket();
    try {
      const [settings, status] = await Promise.all([api.getSettings(), api.getStatus()]);
      const newSettings = claim("settings", t);
      const newStatus = claim("status", t);
      // Apply the theme before the first render of the shell, so it paints in the right colors.
      if (latest.current.kind !== "ready") applyAppearance(settings.settings.appearance);
      setLoad((cur) => {
        if (cur.kind !== "ready") return { kind: "ready", settings, status };
        const next = {
          ...cur,
          settings: newSettings && !same(cur.settings, settings) ? settings : cur.settings,
          status: newStatus && !sameStatus(cur.status, status) ? status : cur.status,
        };
        return next.settings === cur.settings && next.status === cur.status ? cur : next;
      });
    } catch (e) {
      if (latest.current.kind !== "ready") setLoad({ kind: "error", message: errorText(e) });
    }
  }, []);

  const reloadStatus = React.useCallback(async () => {
    const t = ticket();
    try {
      const status = await api.getStatus();
      if (!claim("status", t)) return;
      // Only re-render when something changed, so a poll never moves anything on screen.
      setLoad((cur) => (cur.kind === "ready" && !sameStatus(cur.status, status) ? { ...cur, status } : cur));
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

  const settingsDto = ready?.settings ?? null;
  const statusDto = ready?.status ?? null;

  const settingsValue = React.useMemo<AppSettingsState | null>(() => {
    if (!settingsDto) return null;
    return {
      settings: settingsDto,
      categories,
      colorFor,
      reload: fetchAll,
      reloadStatus,
      setSettings: applySettings,
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
        // Through the app-wide write queue, ordered with the Settings pages' own writes.
        applySettings(await savePatch({ appearance: a }));
      },
    };
  }, [settingsDto, categories, colorFor, fetchAll, reloadStatus, applySettings]);

  // The wording depends on both (a pause lives in the settings, the recorder state in the status).
  const statusValue = React.useMemo<AppStatusState | null>(() => {
    if (!settingsDto || !statusDto) return null;
    return {
      status: statusDto,
      today: statusDto.today,
      recorder: describeRecorder(statusDto, settingsDto),
      analysis: describeAnalysis(statusDto, settingsDto),
      problems: problems(statusDto, settingsDto),
    };
  }, [settingsDto, statusDto]);

  // The store is created with the first status and updated after each render that changed it;
  // components that read the status re-render from there.
  const storeRef = React.useRef<StatusStore | null>(null);
  if (!storeRef.current && statusValue) storeRef.current = createStatusStore(statusValue);
  React.useLayoutEffect(() => {
    if (statusValue) storeRef.current?.set(statusValue);
  }, [statusValue]);

  if (!settingsValue || !statusValue || !storeRef.current) {
    return <>{fallback({ error: load.kind === "error" ? load.message : null, retry: () => void fetchAll() })}</>;
  }
  return (
    <SettingsCtx.Provider value={settingsValue}>
      <StatusStoreCtx.Provider value={storeRef.current}>{children}</StatusStoreCtx.Provider>
    </SettingsCtx.Provider>
  );
}
