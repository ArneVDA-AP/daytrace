import { useState } from "react";
import { api, ProviderKind } from "../api";
import { cliCommand } from "./providerInfo";

/** Path of the claude / codex executable (empty = find it automatically), with a check that runs `--version`. */
export function CliPathField({ provider, path, onChange, id }: { provider: ProviderKind; path: string; onChange: (path: string) => void; id: string }) {
  const [state, setState] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const exe = cliCommand(provider);

  async function check() {
    setBusy(true);
    setState(null);
    try {
      const found = await api.providerCheckCli(provider, path);
      setState({ ok: true, text: `Found ${exe} ${found.version}`.trimEnd() + ` at ${found.path}` });
    } catch (e) {
      setState({ ok: false, text: String(e) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <label className="field" htmlFor={id}>
        <span>Path to {exe}</span>
        <div className="row">
          <input
            id={id}
            value={path}
            placeholder={`Find ${exe} automatically`}
            onChange={(e) => {
              onChange(e.target.value);
              setState(null);
            }}
            spellCheck={false}
          />
          <button className="ghost" type="button" onClick={check} disabled={busy}>
            {busy ? "Checking…" : `Find ${exe}`}
          </button>
        </div>
      </label>
      {state && (
        <p className={state.ok ? "ok" : "bad"} role="status">
          {state.text}
        </p>
      )}
    </>
  );
}
