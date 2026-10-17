import { useEffect, useState } from "react";
import { api, PromptOverrides, ProviderKind } from "../api";
import type { SectionProps } from "./types";
import { CliPathField } from "./CliPathField";
import { PROVIDERS, providerInfo } from "./providerInfo";

/** Fallback provider (Dayflow's secondary provider) and the editable prompt instructions. */
export function ProviderAdvanced({ s, set }: SectionProps) {
  return (
    <>
      <FallbackProvider s={s} set={set} />
      <PromptEditor s={s} set={set} />
    </>
  );
}

function FallbackProvider({ s, set }: SectionProps) {
  const kind = s.fallbackProvider;
  const info = kind ? providerInfo(kind) : null;
  const [key, setKey] = useState("");
  const [hasKey, setHasKey] = useState<boolean | null>(null);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState<"key" | "test" | null>(null);

  // The key slot depends on the provider and, for generic endpoints, on the server.
  useEffect(() => {
    let live = true;
    setHasKey(null);
    if (kind && info?.key !== "none") {
      api
        .providerKeyStatus(kind, s.fallbackBaseUrl)
        .then((v) => live && setHasKey(v))
        .catch(() => live && setHasKey(false));
    }
    return () => {
      live = false;
    };
  }, [kind, s.fallbackBaseUrl, info?.key]);

  async function choose(value: string) {
    setMessage(null);
    setKey("");
    if (value === "") {
      set("fallbackProvider", null);
      return;
    }
    const next = value as ProviderKind;
    const [baseUrl, model] = await api.providerDefaults(next);
    set("fallbackProvider", next);
    set("fallbackBaseUrl", baseUrl);
    set("fallbackModel", model);
  }

  async function saveKey() {
    if (!kind) return;
    setBusy("key");
    setMessage(null);
    try {
      await api.providerSaveKey(kind, s.fallbackBaseUrl, key);
      setKey("");
      setHasKey(true);
      setMessage({ ok: true, text: "Key saved in Windows Credential Manager." });
    } catch (e) {
      setMessage({ ok: false, text: `Couldn’t save the key: ${e}` });
    } finally {
      setBusy(null);
    }
  }

  async function test() {
    if (!kind) return;
    setBusy("test");
    setMessage(null);
    try {
      const out = await api.testProvider(kind, s.fallbackBaseUrl, s.fallbackModel, key || undefined);
      setMessage({ ok: true, text: `The fallback model answered: ${out}` });
    } catch (e) {
      setMessage({ ok: false, text: String(e) });
    } finally {
      setBusy(null);
    }
  }

  const same = kind === s.provider && s.fallbackBaseUrl.trim() === s.baseUrl.trim() && s.fallbackModel.trim() === s.model.trim();

  return (
    <div className="provider-fallback">
      <h3>Fallback provider</h3>
      <p className="hint">
        When the main provider fails a step (for example because it is offline, signed out or out of usage), Daytrace runs that step once on this
        provider and uses it for the rest of that batch. It uses the Detailed mode, or Lightweight for Ollama and LM Studio.
      </p>
      <label className="field" htmlFor="fallback-provider">
        <span>Fallback</span>
        <select id="fallback-provider" value={kind ?? ""} onChange={(e) => choose(e.target.value)}>
          <option value="">None</option>
          {PROVIDERS.map((p) => (
            <option key={p.id} value={p.id}>{p.label}</option>
          ))}
        </select>
      </label>
      {kind && info && (
        <>
          {same && <p className="bad">This is the same provider and model as the main one, so it won’t be used. Pick another model or provider.</p>}
          {info.cli ? (
            <CliPathField id="fallback-cli-path" provider={kind} path={s.fallbackBaseUrl} onChange={(v) => set("fallbackBaseUrl", v)} />
          ) : (
            <label className="field" htmlFor="fallback-url">
              <span>Server address</span>
              <input id="fallback-url" value={s.fallbackBaseUrl} onChange={(e) => set("fallbackBaseUrl", e.target.value)} spellCheck={false} />
            </label>
          )}
          <label className="field" htmlFor="fallback-model">
            <span>Model</span>
            <input id="fallback-model" value={s.fallbackModel} onChange={(e) => set("fallbackModel", e.target.value)} spellCheck={false} />
          </label>
          {info.key !== "none" && (
            <label className="field" htmlFor="fallback-key">
              <span>API key</span>
              <div className="row">
                <input
                  id="fallback-key"
                  type="password"
                  value={key}
                  autoComplete="off"
                  placeholder={
                    hasKey === null ? "Checking…" : hasKey ? "Saved in Windows Credential Manager. Type to replace." : info.key === "optional" ? "Optional" : "Paste the API key"
                  }
                  onChange={(e) => setKey(e.target.value)}
                />
                <button className="ghost" type="button" onClick={saveKey} disabled={busy !== null || !key.trim()}>
                  {busy === "key" ? "Saving…" : "Save key"}
                </button>
              </div>
            </label>
          )}
          {info.key === "required" && hasKey === false && !key && (
            <p className="hint">Without a key this fallback is skipped.</p>
          )}
          <div className="row">
            <button className="ghost" type="button" onClick={test} disabled={busy !== null}>
              {busy === "test" ? "Testing…" : "Test fallback"}
            </button>
            {message && <span className={message.ok ? "ok" : "bad"} role="status">{message.text}</span>}
          </div>
          <p className="hint">Save settings to start using the fallback.</p>
        </>
      )}
    </div>
  );
}

const BLOCKS: { key: keyof PromptOverrides; label: string; help: string }[] = [
  { key: "detailedTitle", label: "Detailed mode: titles", help: "How the model writes card titles." },
  { key: "detailedSummary", label: "Detailed mode: summaries", help: "How the model writes card summaries." },
  { key: "lightweightTitle", label: "Lightweight mode: titles", help: "The title instructions for small local models." },
  { key: "lightweightSummary", label: "Lightweight mode: summaries", help: "The summary guidelines and examples for small local models." },
];

function PromptEditor({ s, set }: SectionProps) {
  const [defaults, setDefaults] = useState<PromptOverrides | null>(null);
  const [error, setError] = useState<string | null>(null);
  const po = s.promptOverrides;
  const edited = BLOCKS.filter((b) => po[b.key].trim() !== "").length;

  useEffect(() => {
    api
      .providerPromptDefaults()
      .then(setDefaults)
      .catch((e) => setError(String(e)));
  }, []);

  function update(k: keyof PromptOverrides, value: string) {
    // Text equal to the default is stored as "no override", so later default updates still apply.
    const stored = defaults && value.trim() === defaults[k].trim() ? "" : value;
    set("promptOverrides", { ...po, [k]: stored });
  }

  return (
    <details className="prompt-editor">
      <summary>
        Prompt instructions{edited > 0 ? ` (${edited} edited)` : ""}
      </summary>
      <p className="hint">
        Change how the model writes card titles and summaries. These instructions are part of the Detailed and Lightweight prompts, also when they run on
        the fallback provider. The one-step prompt of the Claude Code and Codex CLI providers can’t be edited. Daytrace writes each card’s step-by-step
        log from the screenshot transcription, so there is no instruction for it.
      </p>
      {error && <p className="bad">Couldn’t load the default instructions: {error}</p>}
      {!defaults && !error && <p className="hint">Loading the default instructions…</p>}
      {defaults &&
        BLOCKS.map((b) => {
          const custom = po[b.key].trim() !== "";
          const id = `prompt-${b.key}`;
          return (
            <div className="prompt-block" key={b.key}>
              <div className="row">
                <label htmlFor={id}>
                  <b>{b.label}</b> {custom ? "(edited)" : "(default)"}
                </label>
                {custom && (
                  <button className="link" type="button" onClick={() => set("promptOverrides", { ...po, [b.key]: "" })}>
                    Reset to default
                  </button>
                )}
              </div>
              <p className="hint">{b.help}</p>
              <textarea id={id} rows={8} value={custom ? po[b.key] : defaults[b.key]} onChange={(e) => update(b.key, e.target.value)} spellCheck={false} />
            </div>
          );
        })}
      {defaults && edited > 0 && (
        <button
          className="ghost"
          type="button"
          onClick={() => set("promptOverrides", { detailedTitle: "", detailedSummary: "", lightweightTitle: "", lightweightSummary: "" })}
        >
          Reset all to default
        </button>
      )}
    </details>
  );
}
