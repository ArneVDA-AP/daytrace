import { useEffect, useRef, useState } from "react";
import { api, type Pipeline, type ProviderKind, type Settings, type SettingsDto } from "../api";
import { platformApi, withLiveRecordingState } from "./platform-api";
import { PrivateAppPicker, addPrivateApp } from "../settings/PrivateAppPicker";
import { CliPathField } from "../settings/CliPathField";
import { cliSignIn, PROVIDERS, providerInfo } from "../settings/providerInfo";
import "./platform.css";

// First-run setup, after Dayflow's onboarding (Views/Onboarding/OnboardingFlow.swift): how it
// works, the model provider with a connection test, privacy, then start. Skippable; Settings can
// open it again.

// The providers are the shared list Settings uses (settings/providerInfo.ts); these are the
// setup's one-line explanations.
const HINTS: Record<ProviderKind, string> = {
  anthropic: "Best cards. Screenshots are sent to Anthropic under your API key.",
  claude_cli: "Uses your Claude subscription through Claude Code on this PC. Screenshots are sent to Anthropic under that account.",
  ollama: "Free and private: nothing leaves your PC. Needs a vision model such as qwen3-vl:4b.",
  lm_studio: "Free and private, like Ollama.",
  openai_compatible: "OpenAI, OpenRouter, Gemini’s OpenAI endpoint and similar.",
  codex_cli: "Uses your ChatGPT account through the Codex CLI on this PC. Screenshots are sent to OpenAI under that account.",
};

const STEPS = ["How it works", "Model", "Privacy", "Start"] as const;

export function Onboarding({ dto, firstRun, onDone }: { dto: SettingsDto; firstRun: boolean; onDone: () => void }) {
  const [step, setStep] = useState(0);
  const [s, setS] = useState<Settings>(structuredClone(dto.settings));
  const [apiKey, setApiKey] = useState("");
  const [hasKey, setHasKey] = useState(dto.hasApiKey);
  const [models, setModels] = useState<string[]>([]);
  const [test, setTest] = useState<{ ok: boolean; text: string } | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [damagedCopy, setDamagedCopy] = useState<string | null>(null);
  const heading = useRef<HTMLHeadingElement>(null);

  useEffect(() => {
    heading.current?.focus();
  }, [step]);

  // An unreadable settings file brings the setup back; say so.
  useEffect(() => {
    platformApi
      .info()
      .then((i) => setDamagedCopy(i.damagedSettingsCopy))
      .catch(() => {});
  }, []);

  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => setS((p) => ({ ...p, [k]: v }));
  const provider = providerInfo(s.provider);
  const local = s.provider === "ollama" || s.provider === "lm_studio";

  async function changeProvider(id: ProviderKind) {
    const [baseUrl, model] = await api.providerDefaults(id);
    const pipeline: Pipeline = id === "ollama" || id === "lm_studio" ? "local" : "cloud";
    setS((p) => ({ ...p, provider: id, baseUrl, model, pipeline }));
    // A key typed for the previous provider is not saved under the new one.
    setApiKey("");
    setModels([]);
    setTest(null);
  }

  async function save(): Promise<boolean> {
    setBusy("save");
    setError(null);
    try {
      const d = await api.saveSettings(await withLiveRecordingState(s), apiKey ? apiKey : undefined);
      setS(structuredClone(d.settings));
      setHasKey(d.hasApiKey);
      setApiKey("");
      return true;
    } catch (e) {
      setError(`Couldn’t save: ${e}`);
      return false;
    } finally {
      setBusy(null);
    }
  }

  async function next() {
    if (step > 0 && !(await save())) return;
    setStep((n) => Math.min(n + 1, STEPS.length - 1));
  }

  async function finish(recording: boolean) {
    if (!(await save())) return;
    setBusy("finish");
    try {
      await platformApi.finishOnboarding(recording);
      onDone();
    } catch (e) {
      setError(String(e));
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

  const keyMissing = provider.key === "required" && !hasKey && !apiKey.trim();

  return (
    <div className="onboarding">
      <div className="onboarding-inner">
        <ol className="onboarding-steps" aria-label="Setup steps">
          {STEPS.map((name, i) => (
            <li key={name} aria-current={i === step ? "step" : undefined}>
              {i + 1}. {name}
            </li>
          ))}
        </ol>

        {step === 0 && (
          <>
            <h1 tabIndex={-1} ref={heading}>{firstRun ? "Welcome to Daytrace" : "How Daytrace works"}</h1>
            {damagedCopy && (
              <p className="bad" role="alert">
                Daytrace could not read its settings file, so the setup starts again with the default settings. Recording waits until you finish it. The
                unreadable file was kept as {damagedCopy}.
              </p>
            )}
            <p className="lead">Daytrace builds a timeline of your day from screenshots, so you can see where the time went without tracking it yourself.</p>
            <div className="onboarding-cards">
              <article>
                <h2>Install and forget</h2>
                <p>Daytrace takes a screenshot every {s.captureIntervalSecs} seconds in the background and pauses while you are away or the PC is locked. Pause it whenever you like from the tray.</p>
              </article>
              <article>
                <h2>Private by default</h2>
                <p>Screenshots stay on this PC and are deleted after {s.retentionDays === 1 ? "1 day" : `${s.retentionDays} days`}. With a local model, nothing leaves your PC at all.</p>
              </article>
              <article>
                <h2>Understand your day</h2>
                <p>Every 15 minutes a vision model turns the screenshots into timeline cards: what you worked on, for how long, and what distracted you.</p>
              </article>
            </div>
            <section className="onboarding-leaves" aria-labelledby="leaves-h">
              <h2 id="leaves-h">What leaves your PC</h2>
              <ul>
                <li>Every 15 minutes, a sample of that period’s screenshots goes to the model provider you pick in the next step, to write the cards.</li>
                <li>With Claude, ChatGPT or another cloud service, those screenshots are sent to that company under your API key or account.</li>
                <li>With Ollama or LM Studio, the model runs on this PC and nothing is sent.</li>
                <li>Apps on your private list are never captured.</li>
                <li>Once a day Daytrace asks GitHub whether a new version exists. You can turn that off in Settings.</li>
              </ul>
              <p className="hint" style={{ marginTop: 8, marginBottom: 0 }}>Screenshots, the timeline and logs are stored in {dto.dataDir}</p>
            </section>
          </>
        )}

        {step === 1 && (
          <>
            <h1 tabIndex={-1} ref={heading}>Choose the model</h1>
            <p className="lead">This model reads the screenshots and writes the cards.</p>
            <fieldset className="field">
              <span>Provider</span>
              <div className="choices">
                {PROVIDERS.map((p) => (
                  <label key={p.id}>
                    <input type="radio" name="provider" checked={s.provider === p.id} onChange={() => changeProvider(p.id)} />
                    <b>{p.label}</b> {HINTS[p.id]}
                  </label>
                ))}
              </div>
            </fieldset>
            {provider.cli ? (
              <>
                <CliPathField id="onboarding-cli-path" provider={s.provider} path={s.baseUrl} onChange={(v) => set("baseUrl", v)} />
                <p className="hint">Sign in once: {cliSignIn(s.provider)}. Leave the path empty unless Daytrace can’t find it.</p>
              </>
            ) : (
              s.provider !== "anthropic" && (
                <label className="field">
                  <span>Server address</span>
                  <input value={s.baseUrl} onChange={(e) => set("baseUrl", e.target.value)} spellCheck={false} />
                </label>
              )
            )}
            <label className="field">
              <span>Model</span>
              <div className="row">
                <input value={s.model} list="onboarding-models" onChange={(e) => set("model", e.target.value)} spellCheck={false} />
                <datalist id="onboarding-models">{models.map((m) => <option key={m} value={m} />)}</datalist>
                <button className="ghost" type="button" onClick={loadModels} disabled={busy !== null}>
                  {busy === "models" ? "Loading…" : provider.cli ? "Show suggested models" : "Show available models"}
                </button>
              </div>
            </label>
            {provider.key !== "none" && (
              <label className="field">
                <span>API key{provider.key === "optional" ? " (if set)" : ""}</span>
                <input
                  type="password"
                  value={apiKey}
                  placeholder={hasKey ? "Saved in Windows Credential Manager. Type to replace." : "Paste your API key"}
                  onChange={(e) => setApiKey(e.target.value)}
                  autoComplete="off"
                />
              </label>
            )}
            {local && <p className="hint">Local models use the Lightweight analysis: each screenshot is described on its own, which suits small vision models.</p>}
            <div className="row">
              <button className="ghost" type="button" onClick={runTest} disabled={busy !== null}>
                {busy === "test" ? "Testing…" : "Test connection"}
              </button>
              {test && <span className={test.ok ? "ok" : "bad"} role="status">{test.text}</span>}
            </div>
            {keyMissing && <p className="hint">Without a key Daytrace still records; the cards are written once you add one in Settings.</p>}
          </>
        )}

        {step === 2 && (
          <>
            <h1 tabIndex={-1} ref={heading}>Privacy</h1>
            <p className="lead">When one of these apps is in front, Daytrace takes no screenshot; when it is visible behind other windows, it is blanked. Matches part of the program name, one per line.</p>
            <textarea
              rows={7}
              aria-label="Private apps"
              value={s.blockedApps.join("\n")}
              onChange={(e) => set("blockedApps", e.target.value.split("\n").map((l) => l.trim()).filter((l, i, a) => l || i === a.length - 1))}
              spellCheck={false}
            />
            <PrivateAppPicker blocked={s.blockedApps} onAdd={(name) => set("blockedApps", addPrivateApp(s.blockedApps, name))} />
            <label className="check">
              <input type="checkbox" checked={s.launchAtLogin} onChange={(e) => set("launchAtLogin", e.target.checked)} />
              Start Daytrace when I sign in to Windows
            </label>
          </>
        )}

        {step === 3 && (
          <>
            <h1 tabIndex={-1} ref={heading}>{firstRun ? "You’re ready to go" : "All set"}</h1>
            <p className="lead">
              To get useful insights, let Daytrace run in the background for an hour or two to gather enough context, then check back in. Closing the window
              keeps Daytrace running in the tray; pause or quit it from there.
            </p>
            {keyMissing && <p className="bad">No API key is saved yet: screenshots wait until you add one in Settings.</p>}
          </>
        )}

        {error && <p className="bad" role="alert">{error}</p>}

        <div className="onboarding-foot">
          {firstRun ? (
            <button className="link" type="button" onClick={() => finish(true)} disabled={busy !== null}>
              Skip setup and start recording
            </button>
          ) : (
            <button className="link" type="button" onClick={onDone} disabled={busy !== null}>
              Close
            </button>
          )}
          <span className="spacer" />
          {step > 0 && (
            <button className="ghost" type="button" onClick={() => setStep((n) => n - 1)} disabled={busy !== null}>
              Back
            </button>
          )}
          {step < STEPS.length - 1 && (
            <button className="primary" type="button" onClick={next} disabled={busy !== null}>
              {busy === "save" ? "Saving…" : "Continue"}
            </button>
          )}
          {step === STEPS.length - 1 &&
            (firstRun ? (
              <>
                <button className="ghost" type="button" onClick={() => finish(false)} disabled={busy !== null}>
                  Finish without recording
                </button>
                <button className="primary" type="button" onClick={() => finish(true)} disabled={busy !== null}>
                  Start recording
                </button>
              </>
            ) : (
              <button className="primary" type="button" onClick={async () => (await save()) && onDone()} disabled={busy !== null}>
                Done
              </button>
            ))}
        </div>
      </div>
    </div>
  );
}
