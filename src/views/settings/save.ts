import * as React from "react";
import type { SettingsDto } from "@/api";
import { useAppState } from "@/app/state";
import { toast } from "@/components/ui/toast";
import { errorText } from "@/lib/utils";

/*
 * Settings apply as you change them (Windows Settings convention; see the notes in
 * scratchpad/agents/v-settings.md for why). Every change is a small patch saved through the
 * app-wide write queue in app/settings-writes.ts (also used by the rail's theme switch).
 */

import { savePatch, type Patch } from "@/app/settings-writes";

export { afterSaves, savePatch, savedSettings, type Patch } from "@/app/settings-writes";

// ---- save status, shown once in the page header ----

export type SaveState = { kind: "idle" } | { kind: "saving" } | { kind: "saved" } | { kind: "error"; message: string };

interface StatusApi {
  state: SaveState;
  begin: () => void;
  end: (error: string | null) => void;
}

const StatusCtx = React.createContext<StatusApi | null>(null);

export function useSaveStatusValue(): StatusApi {
  const [state, setState] = React.useState<SaveState>({ kind: "idle" });
  const pending = React.useRef(0);
  const failed = React.useRef<string | null>(null);
  const timer = React.useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  React.useEffect(() => () => clearTimeout(timer.current), []);
  return React.useMemo<StatusApi>(
    () => ({
      state,
      begin: () => {
        clearTimeout(timer.current);
        if (pending.current === 0) failed.current = null;
        pending.current += 1;
        setState({ kind: "saving" });
      },
      end: (error) => {
        pending.current = Math.max(0, pending.current - 1);
        if (error) failed.current = error;
        if (pending.current > 0) return;
        if (failed.current) {
          setState({ kind: "error", message: failed.current });
          return;
        }
        setState({ kind: "saved" });
        timer.current = setTimeout(() => setState({ kind: "idle" }), 4000);
      },
    }),
    [state],
  );
}

export const SaveStatusProvider = StatusCtx.Provider;

export function useSaveState(): SaveState {
  return React.useContext(StatusCtx)?.state ?? { kind: "idle" };
}

/**
 * `save(patch, { apiKey })`: saves, puts the result into the app state and reports progress to the
 * header. Rejects with the backend's message, so the control that started it can show it.
 */
export function useSave() {
  const app = useAppState();
  const status = React.useContext(StatusCtx);
  const latest = React.useRef({ app, status });
  latest.current = { app, status };
  return React.useCallback(async (patch: Patch, opts: { apiKey?: string } = {}): Promise<SettingsDto> => {
    latest.current.status?.begin();
    try {
      const dto = await savePatch(patch, opts.apiKey);
      latest.current.app.setSettings(dto);
      latest.current.status?.end(null);
      return dto;
    } catch (e) {
      latest.current.status?.end(errorText(e));
      throw e;
    }
  }, []);
}

// ---- text fields: edit freely, save on Enter or when focus leaves, and when leaving the page ----

export interface CommitField {
  /** What the field shows: the text being edited, or the saved value. */
  value: string;
  /** Typed but not saved yet. */
  dirty: boolean;
  /** A validation message or the backend's refusal. */
  error: string | null;
  saving: boolean;
  set: (v: string) => void;
  /** Saves the typed text now (no-op when nothing changed). True when nothing is left unsaved. */
  commit: () => Promise<boolean>;
  /** Drops the typed text. */
  revert: () => void;
  /** Props for an <input>/<textarea>: value, onChange, onBlur, Enter/Escape, aria-invalid. */
  bind: {
    value: string;
    onChange: (e: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => void;
    onBlur: () => void;
    onKeyDown: (e: React.KeyboardEvent<HTMLInputElement | HTMLTextAreaElement>) => void;
    "aria-invalid": true | undefined;
  };
}

/**
 * A text setting. Typing keeps a local draft; Enter (Ctrl+Enter in a text area) or leaving the
 * field saves it, Escape drops it. A draft left when the component unmounts (leaving Settings, a
 * section switch, a shortcut) is saved then, unless it is invalid: leaving never loses typing.
 */
export function useCommitField(
  saved: string,
  onCommit: (v: string) => Promise<unknown>,
  opts: { validate?: (v: string) => string | null; multiline?: boolean } = {},
): CommitField {
  const [draft, setDraft] = React.useState<string | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [saving, setSaving] = React.useState(false);
  const live = React.useRef({ draft, saved, onCommit, validate: opts.validate });
  live.current = { draft, saved, onCommit, validate: opts.validate };

  const commit = React.useCallback(async (): Promise<boolean> => {
    const { draft: d, saved: s, onCommit: save, validate } = live.current;
    if (d === null) return true;
    if (d === s) {
      setDraft(null);
      setError(null);
      return true;
    }
    const invalid = validate?.(d) ?? null;
    if (invalid) {
      setError(invalid);
      return false;
    }
    setSaving(true);
    try {
      await save(d);
      // Only clear when nothing was typed meanwhile.
      if (live.current.draft === d) setDraft(null);
      setError(null);
      return true;
    } catch (e) {
      setError(errorText(e));
      return false;
    } finally {
      setSaving(false);
    }
  }, []);

  // Leaving with typed text: save it (unless invalid; the message was already on screen).
  React.useEffect(
    () => () => {
      const { draft: d, saved: s, onCommit: save, validate } = live.current;
      if (d === null || d === s || validate?.(d)) return;
      void save(d).catch((e) => toast("A setting couldn’t be saved", { description: errorText(e), tone: "error" }));
    },
    [],
  );

  const set = (v: string) => {
    setDraft(v);
    setError(opts.validate?.(v) ?? null);
  };
  const revert = () => {
    setDraft(null);
    setError(null);
  };

  const value = draft ?? saved;
  return {
    value,
    dirty: draft !== null && draft !== saved,
    error,
    saving,
    set,
    commit,
    revert,
    bind: {
      value,
      onChange: (e) => set(e.target.value),
      onBlur: () => void commit(),
      onKeyDown: (e) => {
        if (e.key === "Enter" && (!opts.multiline || e.ctrlKey) && !e.nativeEvent.isComposing) {
          e.preventDefault();
          void commit();
        } else if (e.key === "Escape" && draft !== null) {
          e.preventDefault();
          e.stopPropagation();
          revert();
        }
      },
      "aria-invalid": error ? true : undefined,
    },
  };
}
