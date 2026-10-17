import { useState } from "react";
import { api, Category, Pipeline, ProviderKind, Settings, SettingsDto } from "./api";
import { ProviderAdvanced } from "./settings/ProviderAdvanced";
import { RecapSection } from "./settings/RecapSection";
import { PlatformSection } from "./settings/PlatformSection";

const PROVIDERS: { id: ProviderKind; label: string; needsKey: boolean }[] = [
  { id: "anthropic", label: "Claude (Anthropic API)", needsKey: true },
  { id: "ollama", label: "Ollama (on this PC)", needsKey: false },
  { id: "lm_studio", label: "LM Studio (on this PC)", needsKey: false },
  { id: "openai_compatible", label: "Other OpenAI-compatible API", needsKey: true },
];

export function SettingsView({ dto, onSaved, onClose }: { dto: SettingsDto; onSaved: (d: SettingsDto) => void; onClose: () => void }) {
  const [s, setS] = useState<Settings>(structuredClone(dto.settings));
  const [apiKey, setApiKey] = useState("");
  const [models, setModels] = useState<string[]>([]);
  const [test, setTest] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const [hasKey, setHasKey] = useState(dto.hasApiKey);

  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => {
    setS((p) => ({ ...p, [k]: v }));
    setSaved(false);
  };
  const provider = PROVIDERS.find((p) => p.id === s.provider)!;

  async function changeProvider(id: ProviderKind) {
    const [baseUrl, model] = await api.providerDefaults(id);
    const pipeline: Pipeline = id === "ollama" || id === "lm_studio" ? "local" : "cloud";
    setS((p) => ({ ...p, provider: id, baseUrl, model, pipeline }));
    setModels([]);
    setTest(null);
    setSaved(false);
  }

  async function loadModels() {
    setBusy("models");
    try {
      setModels(await api.listModels(s.provider, s.baseUrl, apiKey));
    } catch (e) {
      setTest({ ok: false, text: `Couldn’t list models: ${e}` });
    } finally {
      setBusy(null);
    }
  }

  async function runTest() {
    setBusy("test");
    setTest(null);
    try {
      const out = await api.testProvider(s.provider, s.baseUrl, s.model, apiKey);
      setTest({ ok: true, text: `The model answered: ${out}` });
    } catch (e) {
      setTest({ ok: false, text: String(e) });
    } finally {
      setBusy(null);
    }
  }

  async function save() {
    setBusy("save");
    try {
      const d = await api.saveSettings(s, apiKey ? apiKey : undefined);
      setHasKey(d.hasApiKey);
      setApiKey("");
      setS(structuredClone(d.settings));
      setSaved(true);
      onSaved(d);
    } catch (e) {
      setTest({ ok: false, text: `Couldn’t save: ${e}` });
    } finally {
      setBusy(null);
    }
  }

  function updateCat(i: number, patch: Partial<Category>) {
    set("categories", s.categories.map((c, j) => (j === i ? { ...c, ...patch } : c)));
  }

  return (
    <div className="settings">
      <div className="settings-inner">
        <section>
          <h2>Model</h2>
          <p className="hint">Screenshots are sent to this model every 15 minutes. With Ollama or LM Studio nothing leaves your PC.</p>
          <label className="field">
            <span>Provider</span>
            <select value={s.provider} onChange={(e) => changeProvider(e.target.value as ProviderKind)}>
              {PROVIDERS.map((p) => (
                <option key={p.id} value={p.id}>{p.label}</option>
              ))}
            </select>
          </label>
          <label className="field">
            <span>Server address</span>
            <input value={s.baseUrl} onChange={(e) => set("baseUrl", e.target.value)} spellCheck={false} />
          </label>
          <label className="field">
            <span>Model</span>
            <div className="row">
              <input value={s.model} list="models" onChange={(e) => set("model", e.target.value)} spellCheck={false} />
              <datalist id="models">{models.map((m) => <option key={m} value={m} />)}</datalist>
              <button className="ghost" onClick={loadModels} disabled={busy !== null}>
                {busy === "models" ? "Loading…" : "Show available models"}
              </button>
            </div>
          </label>
          {(provider.needsKey || s.provider === "lm_studio") && (
            <label className="field">
              <span>API key</span>
              <input
                type="password"
                value={apiKey}
                placeholder={hasKey ? "Saved in Windows Credential Manager. Type to replace." : "Paste your API key"}
                onChange={(e) => {
                  setApiKey(e.target.value);
                  setSaved(false);
                }}
                autoComplete="off"
              />
            </label>
          )}
          <fieldset className="field">
            <span>Analysis</span>
            <div className="choices">
              <label>
                <input type="radio" checked={s.pipeline === "cloud"} onChange={() => set("pipeline", "cloud")} />
                <b>Detailed</b> One request with 15 screenshots, then the last 45 minutes are rewritten as a whole. Best with Claude or other large models.
              </label>
              <label>
                <input type="radio" checked={s.pipeline === "local"} onChange={() => set("pipeline", "local")} />
                <b>Lightweight</b> Each screenshot is described on its own, then combined into one card. Suited to small local vision models such as qwen3-vl:4b.
              </label>
            </div>
          </fieldset>
          {s.provider === "anthropic" && (
            <label className="field">
              <span>Model reasoning</span>
              <select value={s.reasoning} onChange={(e) => set("reasoning", e.target.value as Settings["reasoning"])}>
                <option value="low">Low (recommended)</option>
                <option value="off">Off (cheapest, breaks Dayflow’s card rules more often)</option>
                <option value="default">Model default (about 1.5× the output cost of Low)</option>
              </select>
            </label>
          )}
          {s.provider === "anthropic" && (
            <label className="check">
              <input type="checkbox" checked={s.useBatchApi} onChange={(e) => set("useBatchApi", e.target.checked)} />
              Use the Batch API: half the cost, but cards can take from a few minutes up to an hour to appear
            </label>
          )}
          <div className="row">
            <button className="ghost" onClick={runTest} disabled={busy !== null}>{busy === "test" ? "Testing…" : "Test connection"}</button>
            {test && <span className={test.ok ? "ok" : "bad"}>{test.text}</span>}
          </div>
          <ProviderAdvanced s={s} set={set} />
        </section>

        <section>
          <h2>Recording</h2>
          <label className="field">
            <span>Take a screenshot every</span>
            <select value={s.captureIntervalSecs} onChange={(e) => set("captureIntervalSecs", Number(e.target.value))}>
              {[5, 10, 20, 30, 60].map((n) => <option key={n} value={n}>{n} seconds</option>)}
            </select>
          </label>
          <label className="field">
            <span>Stop screenshots when I’m away for</span>
            <select value={s.idlePauseSecs} onChange={(e) => set("idlePauseSecs", Number(e.target.value))}>
              <option value={60}>1 minute</option>
              <option value={120}>2 minutes</option>
              <option value={300}>5 minutes</option>
              <option value={600}>10 minutes</option>
              <option value={0}>Never stop</option>
            </select>
          </label>
          <p className="hint">“Away” means no keyboard or mouse input. While a video or call keeps the screen on, Daytrace keeps recording. Time away still shows on the timeline as Idle, without screenshots or model calls.</p>
          <label className="field">
            <span>Screenshot size</span>
            <select value={s.captureHeight} onChange={(e) => set("captureHeight", Number(e.target.value))}>
              <option value={720}>720p (smaller files)</option>
              <option value={1080}>1080p (easier to read)</option>
            </select>
          </label>
          <label className="field">
            <span>Keep screenshots for</span>
            <select value={s.retentionDays} onChange={(e) => set("retentionDays", Number(e.target.value))}>
              {[1, 3, 7, 14, 30, 90].map((n) => <option key={n} value={n}>{n === 1 ? "1 day" : `${n} days`}</option>)}
            </select>
          </label>
          <p className="hint">
            Timeline cards and the text descriptions of your screen are kept after their screenshots are deleted. The names of the apps and windows you
            used are kept for a year.
          </p>
          <label className="check">
            <input type="checkbox" checked={s.launchAtLogin} onChange={(e) => set("launchAtLogin", e.target.checked)} />
            Start Daytrace when I sign in to Windows
          </label>
        </section>

        <section>
          <h2>Private apps</h2>
          <p className="hint">When one of these apps is in front, Daytrace skips the screenshot. Matches part of the program name, one per line.</p>
          <textarea
            rows={6}
            value={s.blockedApps.join("\n")}
            onChange={(e) => set("blockedApps", e.target.value.split("\n").map((l) => l.trim()).filter((l, i, a) => l || i === a.length - 1))}
            spellCheck={false}
          />
        </section>

        <section>
          <h2>Categories</h2>
          <p className="hint">The model picks one category for each card, using these descriptions.</p>
          <div className="cats">
            {s.categories.map((c, i) => (
              <div className="cat" key={i}>
                <input type="color" value={c.color} onChange={(e) => updateCat(i, { color: e.target.value })} aria-label={`${c.name} color`} />
                <input value={c.name} onChange={(e) => updateCat(i, { name: e.target.value })} disabled={c.isIdle} aria-label="Name" />
                <input
                  value={c.isIdle ? "Used when you were away for most of a period" : c.description}
                  onChange={(e) => updateCat(i, { description: e.target.value })}
                  disabled={c.isIdle}
                  aria-label="Description"
                />
                {!c.isIdle && (
                  <button className="link" onClick={() => set("categories", s.categories.filter((_, j) => j !== i))}>Remove</button>
                )}
              </div>
            ))}
          </div>
          <button
            className="ghost"
            onClick={() => {
              const idle = s.categories.findIndex((c) => c.isIdle);
              const next = [...s.categories];
              next.splice(idle < 0 ? next.length : idle, 0, { name: "New category", color: "#7FB77E", description: "", isIdle: false });
              set("categories", next);
            }}
          >
            Add category
          </button>
        </section>

        <section>
          <h2>Language</h2>
          <label className="field">
            <span>Write summaries in</span>
            <input value={s.outputLanguage} placeholder="English" onChange={(e) => set("outputLanguage", e.target.value)} />
          </label>
        </section>

        <RecapSection s={s} set={set} />
        <PlatformSection s={s} set={set} />

        <section>
          <h2>Your data</h2>
          <p className="hint">Screenshots, the timeline database and logs are stored in {dto.dataDir}</p>
          <button className="ghost" onClick={() => api.openDataDir()}>Open folder</button>
        </section>
      </div>

      <footer className="settings-foot">
        {saved && <span className="ok">Settings saved</span>}
        <button className="ghost" onClick={onClose}>Back to timeline</button>
        <button className="primary" onClick={save} disabled={busy !== null}>{busy === "save" ? "Saving…" : "Save settings"}</button>
      </footer>
    </div>
  );
}
