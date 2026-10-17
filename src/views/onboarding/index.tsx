import * as React from "react";
import { ArrowLeft, ArrowRight, Check, Lock, MonitorUp, Plus, TriangleAlert } from "lucide-react";
import { api, type ProviderKind, type Settings, type SettingsDto } from "@/api";
import { LogoMark } from "@/app/Logo";
import { useAppState } from "@/app/state";
import { Banner } from "@/components/banner";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { RadioGroup } from "@/components/ui/radio-group";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { cn, errorText } from "@/lib/utils";
import { platformApi, withLiveRecordingState } from "@/views/platform-api";
import { CliPathField } from "@/views/settings/cli-path-field";
import { FieldError, OutcomeLine, type Outcome } from "@/views/settings/fields";
import { addPrivateApps, PRIVATE_APPS_HELP, seenText, useAppCandidates } from "@/views/settings/private-apps";
import { ProviderOption, testText } from "@/views/settings/provider-section";
import { cliProduct, cliSignIn, defaultPipeline, keySource, PROVIDERS, providerInfo } from "@/views/settings/providers";
import { DataFlow, SampleDay } from "./intro-step";

/*
 * First-run setup, after Dayflow's onboarding (Views/Onboarding/OnboardingFlow.swift): what
 * Daytrace does and what leaves the PC, the model provider with a connection test, private apps,
 * then start. It replaces the shell while settings.onboardingCompleted is false, or when Settings
 * or the command menu opens it again (OPEN_ONBOARDING_EVENT). Existing installs never see it (the
 * backend marks them completed at startup).
 *
 * Unlike Settings, the setup is a wizard: changes are kept in a draft and saved when you move to
 * another step, finish, skip or close it, through the same withLiveRecordingState path.
 */

const STEPS = ["What Daytrace does", "Model provider", "Private apps", "Start recording"] as const;
const HEADINGS = ["", "Choose who writes your cards", "Pick apps that are never captured", ""] as const;
const SPINNER = { "aria-hidden": true, role: undefined, "aria-label": undefined } as const;

export default function OnboardingScreen({ firstRun, onDone }: { firstRun: boolean; onDone: () => void }) {
  const app = useAppState();
  return (
    <Onboarding
      dto={app.settings}
      firstRun={firstRun}
      onDone={async () => {
        await app.reload();
        onDone();
      }}
    />
  );
}

function Onboarding({ dto, firstRun, onDone }: { dto: SettingsDto; firstRun: boolean; onDone: () => void }) {
  const [step, setStep] = React.useState(0);
  const [s, setS] = React.useState<Settings>(() => structuredClone(dto.settings));
  const [apiKey, setApiKey] = React.useState("");
  const [hasKey, setHasKey] = React.useState(dto.hasApiKey);
  const [models, setModels] = React.useState<string[]>([]);
  const [test, setTest] = React.useState<Outcome | null>(null);
  const [busy, setBusy] = React.useState<string | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [damagedCopy, setDamagedCopy] = React.useState<string | null>(null);
  const heading = React.useRef<HTMLHeadingElement>(null);
  const scroller = React.useRef<HTMLDivElement>(null);

  React.useEffect(() => {
    heading.current?.focus();
    scroller.current?.scrollTo({ top: 0 });
  }, [step]);

  // An unreadable settings file brings the setup back; say so.
  React.useEffect(() => {
    platformApi
      .info()
      .then((i) => setDamagedCopy(i.damagedSettingsCopy ?? null))
      .catch(() => {});
  }, []);

  const set = <K extends keyof Settings>(k: K, v: Settings[K]) => setS((p) => ({ ...p, [k]: v }));
  const provider = providerInfo(s.provider);
  const keyMissing = provider.key === "required" && !hasKey && !apiKey.trim();

  async function changeProvider(id: ProviderKind) {
    if (id === s.provider) return;
    // A key typed for the previous provider is not saved under the new one.
    setApiKey("");
    setModels([]);
    setTest(null);
    try {
      const [baseUrl, model] = await api.providerDefaults(id);
      setS((p) => ({ ...p, provider: id, baseUrl, model, pipeline: defaultPipeline(id) }));
    } catch (e) {
      setError(errorText(e));
    }
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
      setError(`Couldn’t save: ${errorText(e)}`);
      return false;
    } finally {
      setBusy(null);
    }
  }

  async function goTo(n: number) {
    if (n === step) return;
    if (step > 0 && !(await save())) return;
    setStep(Math.max(0, Math.min(n, STEPS.length - 1)));
  }

  async function finish(recording: boolean) {
    if (!(await save())) return;
    setBusy("finish");
    try {
      await platformApi.finishOnboarding(recording);
      onDone();
    } catch (e) {
      setError(errorText(e));
      setBusy(null);
    }
  }

  async function close() {
    if (step > 0 && !(await save())) return;
    onDone();
  }

  async function runTest() {
    setBusy("test");
    setTest(null);
    try {
      const out = await api.testProvider(s.provider, s.baseUrl, s.model, apiKey);
      setTest({ ok: true, text: testText(out, s.model) });
    } catch (e) {
      setTest({ ok: false, text: errorText(e) });
    } finally {
      setBusy(null);
    }
  }

  async function loadModels() {
    setBusy("models");
    try {
      setModels(await api.listModels(s.provider, s.baseUrl, apiKey));
    } catch (e) {
      setTest({ ok: false, text: `Couldn’t list models: ${errorText(e)}` });
    } finally {
      setBusy(null);
    }
  }

  const nextLabel = step === 0 ? "Set up a provider" : step === 1 ? "Continue to private apps" : "Continue";
  const title =
    step === 0 ? (firstRun ? "Welcome to Daytrace" : "How Daytrace works") : step === 3 ? (firstRun ? "Ready to record" : "All set") : HEADINGS[step];

  return (
    <div className="scroll-thin flex h-full overflow-y-auto bg-background">
      {/* Narrow windows: logo and steps in one row above a card that fills the rest. Wide: steps on
          the left, a card of fixed height, so it doesn't jump between steps. */}
      <div className="mx-auto flex h-full w-full max-w-5xl flex-col gap-4 p-4 split:m-auto split:grid split:h-auto split:grid-cols-[14rem_minmax(0,1fr)] split:gap-8 split:p-6">
        <aside className="flex shrink-0 items-center justify-between gap-4 split:flex-col split:items-stretch split:justify-start split:gap-5 split:pt-2">
          <div className="flex items-center gap-2.5">
            <LogoMark className="size-7" />
            <span className="font-display text-[1.125rem] font-semibold" translate="no">
              Daytrace
            </span>
          </div>
          <ol className="flex gap-1 split:flex-col" aria-label="Setup steps">
            {STEPS.map((name, i) => {
              const state = i < step ? "done" : i === step ? "current" : "todo";
              return (
                <li key={name} className="min-w-0">
                  <button
                    type="button"
                    onClick={() => void goTo(i)}
                    disabled={busy !== null}
                    aria-current={state === "current" ? "step" : undefined}
                    className={cn(
                      "flex w-full items-center gap-2.5 rounded-md px-2 py-1.5 text-left text-meta disabled:opacity-100",
                      state === "current" ? "bg-surface-1 font-medium text-foreground shadow-xs ring-1 ring-border" : "text-muted-foreground hover:text-foreground",
                    )}
                  >
                    <span
                      aria-hidden
                      className={cn(
                        "grid size-5 shrink-0 place-items-center rounded-full border border-border-strong text-caption tnum",
                        state === "done" && "border-foreground bg-foreground text-background",
                        state === "current" && "border-foreground",
                      )}
                    >
                      {state === "done" ? <Check className="size-3" aria-hidden /> : i + 1}
                    </span>
                    <span className={cn("truncate", state !== "current" && "max-split:sr-only")}>
                      <span className="sr-only">Step {i + 1}: </span>
                      {name}
                    </span>
                    {state === "done" && <span className="sr-only"> (done)</span>}
                  </button>
                </li>
              );
            })}
          </ol>
          <p className="hidden text-caption text-muted-foreground split:block">
            Daytrace is an unofficial Windows port of Dayflow. It has no account and no analytics.
          </p>
        </aside>

        <main
          id="content"
          tabIndex={-1}
          aria-label="Setup"
          className="flex min-h-0 flex-1 flex-col rounded-xl border bg-surface-1 shadow-md outline-none split:h-[min(46rem,calc(100dvh-3rem))]"
        >
          <div ref={scroller} className="scroll-thin flex min-h-0 flex-1 flex-col gap-6 overflow-y-auto overscroll-contain px-7 py-6 max-split:px-5">
            <div className="flex flex-col gap-1.5">
              <p className="text-caption text-muted-foreground">
                Step {step + 1} of {STEPS.length} · {STEPS[step]}
              </p>
              <h1 ref={heading} tabIndex={-1} className="font-display text-[1.625rem] leading-8 font-semibold tracking-[-0.015em] outline-none">
                {title}
              </h1>
              <p className="max-w-[62ch] text-body text-muted-foreground">
                {step === 0 && "Daytrace builds a timeline of your day from screenshots, so you can see where the time went without tracking it yourself."}
                {step === 1 && "This model reads the screenshots and writes the timeline cards. You can change it later in Settings."}
                {step === 2 && PRIVATE_APPS_HELP}
                {step === 3 &&
                  "To get useful insights, let Daytrace run in the background for an hour or two to gather enough context, then check back in. The first cards appear about 15 minutes after you start."}
              </p>
            </div>

            {step === 0 && <IntroStep s={s} dataDir={dto.dataDir} damagedCopy={damagedCopy} />}
            {step === 1 && (
              <ProviderStep
                s={s}
                set={set}
                apiKey={apiKey}
                setApiKey={setApiKey}
                hasKey={hasKey}
                keyMissing={keyMissing}
                changeProvider={changeProvider}
                models={models}
                loadModels={loadModels}
                test={test}
                runTest={runTest}
                busy={busy}
              />
            )}
            {step === 2 && <PrivacyStep s={s} set={set} />}
            {step === 3 && <StartStep s={s} set={set} keyMissing={keyMissing} hasKey={hasKey} />}
          </div>

          <div className="flex flex-col gap-3 border-t px-7 py-4 max-split:px-5">
            {error && (
              <p role="alert" className="flex items-start gap-2 text-meta text-destructive">
                <TriangleAlert className="mt-0.5 size-4 shrink-0" aria-hidden />
                <span className="min-w-0 break-words">{error}</span>
              </p>
            )}
            <div className="flex flex-wrap items-center gap-2">
              {firstRun ? (
                step < STEPS.length - 1 && (
                  <Button variant="ghost" className="text-muted-foreground" onClick={() => void finish(true)} disabled={busy !== null}>
                    Skip setup and start recording
                  </Button>
                )
              ) : (
                <Button variant="ghost" className="text-muted-foreground" onClick={() => void close()} disabled={busy !== null}>
                  Close setup
                </Button>
              )}
              <span className="flex-1" />
              {step > 0 && (
                <Button variant="ghost" onClick={() => void goTo(step - 1)} disabled={busy !== null}>
                  <ArrowLeft aria-hidden /> Back
                </Button>
              )}
              {step < STEPS.length - 1 && (
                <Button onClick={() => void goTo(step + 1)} disabled={busy !== null}>
                  {busy === "save" && <Spinner {...SPINNER} />}
                  {busy === "save" ? "Saving…" : nextLabel} {busy !== "save" && <ArrowRight aria-hidden />}
                </Button>
              )}
              {step === STEPS.length - 1 &&
                (firstRun ? (
                  <>
                    <Button variant="outline" onClick={() => void finish(false)} disabled={busy !== null}>
                      Finish without recording
                    </Button>
                    <Button size="lg" onClick={() => void finish(true)} disabled={busy !== null}>
                      {(busy === "save" || busy === "finish") && <Spinner {...SPINNER} />}
                      Start recording
                    </Button>
                  </>
                ) : (
                  <Button onClick={async () => (await save()) && onDone()} disabled={busy !== null}>
                    Done
                  </Button>
                ))}
            </div>
          </div>
        </main>
      </div>
    </div>
  );
}

type SetFn = <K extends keyof Settings>(k: K, v: Settings[K]) => void;

function IntroStep({ s, dataDir, damagedCopy }: { s: Settings; dataDir: string; damagedCopy: string | null }) {
  return (
    <>
      {damagedCopy && (
        <Banner tone="warn" icon={<TriangleAlert aria-hidden />} title="Daytrace couldn’t read its settings file">
          So the setup starts again with the default settings, and recording waits until you finish it. The unreadable file was kept as{" "}
          <span className="font-mono text-caption [overflow-wrap:anywhere]">{damagedCopy}</span>.
        </Banner>
      )}
      <DataFlow s={s} />
      <SampleDay />
      <section aria-labelledby="know-h" className="flex flex-col gap-2">
        <h2 id="know-h" className="text-body font-semibold">
          Good to know
        </h2>
        <ul className="flex list-disc flex-col gap-1 pl-5 text-meta text-muted-foreground marker:text-subtle-foreground">
          <li>Daytrace pauses while you are away or the PC is locked. Pause it any time from the tray icon.</li>
          <li>Apps on your private list are never captured. You pick them in step 3.</li>
          <li>Chat and the daily recap send the text of your cards to the model, never screenshots.</li>
          <li>Once a day Daytrace asks GitHub whether a new version exists. You can turn that off in Settings.</li>
          <li>
            Screenshots, the timeline and logs are stored in{" "}
            <span className="font-mono text-caption text-foreground [overflow-wrap:anywhere]" translate="no">
              {dataDir}
            </span>
          </li>
        </ul>
      </section>
    </>
  );
}

function ProviderStep({
  s,
  set,
  apiKey,
  setApiKey,
  hasKey,
  keyMissing,
  changeProvider,
  models,
  loadModels,
  test,
  runTest,
  busy,
}: {
  s: Settings;
  set: SetFn;
  apiKey: string;
  setApiKey: (v: string) => void;
  hasKey: boolean;
  keyMissing: boolean;
  changeProvider: (id: ProviderKind) => Promise<void>;
  models: string[];
  loadModels: () => Promise<void>;
  test: Outcome | null;
  runTest: () => Promise<void>;
  busy: string | null;
}) {
  const p = providerInfo(s.provider);
  const id = React.useId();
  return (
    <>
      <RadioGroup value={s.provider} onValueChange={(v) => void changeProvider(v as ProviderKind)} aria-label="Model provider" className="grid gap-2 split:grid-cols-2">
        {PROVIDERS.map((o) => (
          <ProviderOption key={o.id} p={o} selected={s.provider === o.id} card />
        ))}
      </RadioGroup>

      <section aria-labelledby={`${id}-h`} className="flex flex-col gap-4 rounded-lg border p-4">
        <h2 id={`${id}-h`} className="text-body font-semibold">
          Set up {p.title}
        </h2>
        {p.cli ? (
          <div className="flex flex-col gap-2">
            <label htmlFor={`${id}-cli`} className="text-meta font-medium">
              Path to {p.id === "codex_cli" ? "codex" : "claude"}
            </label>
            <CliPathField
              id={`${id}-cli`}
              provider={s.provider}
              input={{ value: s.baseUrl, onChange: (e) => set("baseUrl", e.target.value) }}
              describedBy={`${id}-cli-help`}
            />
            <p id={`${id}-cli-help`} className="text-meta text-muted-foreground">
              Install {cliProduct(s.provider)}, then sign in once: {cliSignIn(s.provider)}. Leave the path empty unless Daytrace can’t find it.
              {s.provider === "claude_cli" ? " On Windows, Claude Code also needs Git for Windows." : ""}
            </p>
          </div>
        ) : (
          s.provider !== "anthropic" && (
            <div className="flex flex-col gap-2">
              <label htmlFor={`${id}-url`} className="text-meta font-medium">
                Server address
              </label>
              <Input id={`${id}-url`} value={s.baseUrl} onChange={(e) => set("baseUrl", e.target.value)} spellCheck={false} className="font-mono text-meta" />
            </div>
          )
        )}

        {p.key !== "none" && (
          <div className="flex flex-col gap-2">
            <label htmlFor={`${id}-key`} className="text-meta font-medium">
              API key{p.key === "optional" ? " (optional)" : ""}
            </label>
            <Input
              id={`${id}-key`}
              type="password"
              value={apiKey}
              placeholder={hasKey ? "Saved. Type a new key to replace it." : "Paste your API key"}
              onChange={(e) => setApiKey(e.target.value)}
              autoComplete="off"
              spellCheck={false}
              aria-describedby={`${id}-key-help`}
              className="font-mono text-meta placeholder:font-sans"
            />
            <p id={`${id}-key-help`} className="flex flex-wrap items-center gap-x-1.5 text-meta text-muted-foreground">
              {keySource(s.provider)}{" "}
              <span className="inline-flex items-center gap-1">
                <Lock className="size-3.5" aria-hidden /> Stored in Windows Credential Manager when you continue.
              </span>
            </p>
          </div>
        )}

        <div className="flex flex-col gap-2">
          <label htmlFor={`${id}-model`} className="text-meta font-medium">
            Model
          </label>
          <div className="flex flex-wrap gap-2">
            <Input
              id={`${id}-model`}
              value={s.model}
              list={`${id}-models`}
              onChange={(e) => set("model", e.target.value)}
              spellCheck={false}
              className="min-w-0 flex-1 basis-56 font-mono text-meta"
            />
            <datalist id={`${id}-models`}>{models.map((m) => <option key={m} value={m} />)}</datalist>
            <Button type="button" variant="outline" onClick={() => void loadModels()} disabled={busy !== null}>
              {busy === "models" && <Spinner {...SPINNER} />}
              {busy === "models" ? "Loading…" : p.cli ? "Show suggested models" : "Show available models"}
            </Button>
          </div>
          {p.local && <p className="text-meta text-muted-foreground">Local models use the Lightweight analysis: each screenshot is described on its own, which suits small vision models.</p>}
        </div>

        <div className="flex flex-col gap-2">
          <div className="flex flex-wrap items-center gap-3">
            <Button type="button" variant="secondary" onClick={() => void runTest()} disabled={busy !== null}>
              {busy === "test" && <Spinner {...SPINNER} />}
              {busy === "test" ? "Testing…" : "Test connection"}
            </Button>
            {busy === "test" && p.cli && <span className="text-meta text-muted-foreground">The CLI can take up to a minute.</span>}
          </div>
          <div aria-live="polite">
            <OutcomeLine outcome={test} />
          </div>
          {keyMissing && <p className="text-meta text-muted-foreground">Without a key Daytrace still records; the cards are written once you add one in Settings.</p>}
        </div>
      </section>
    </>
  );
}

function PrivacyStep({ s, set }: { s: Settings; set: SetFn }) {
  const id = React.useId();
  const [draft, setDraft] = React.useState("");
  const { apps, loading, error, load } = useAppCandidates(true);
  // Names shown stay in the list when unticked, so a tick can be put back.
  const [shown, setShown] = React.useState<string[]>(() => [...s.blockedApps]);
  React.useEffect(() => {
    if (!apps) return;
    setShown((cur) => addPrivateApps(cur, apps.map((a) => a.name).join("\n")));
  }, [apps]);

  const blocked = new Set(s.blockedApps.map((b) => b.trim().toLowerCase()));
  const toggle = (name: string, on: boolean) =>
    set("blockedApps", on ? addPrivateApps(s.blockedApps, name) : s.blockedApps.filter((b) => b.trim().toLowerCase() !== name.toLowerCase()));
  const candidate = (name: string) => apps?.find((a) => a.name.toLowerCase() === name.toLowerCase());

  return (
    <>
      <div className="flex flex-col gap-2">
        <div className="flex items-baseline justify-between gap-3">
          <h2 id={`${id}-h`} className="text-body font-semibold">
            Apps on this PC
          </h2>
          <span className="text-caption text-muted-foreground tnum" aria-live="polite">
            {s.blockedApps.length} never captured
          </span>
        </div>
        <ul aria-labelledby={`${id}-h`} className="flex flex-col divide-y rounded-lg border">
          {shown.map((name) => {
            const c = candidate(name);
            const on = blocked.has(name.trim().toLowerCase());
            return (
              <li key={name.toLowerCase()}>
                <label className="flex items-center gap-3 px-4 py-2 hover:bg-surface-2/60">
                  <Checkbox checked={on} onCheckedChange={(v) => toggle(name, v === true)} />
                  <span className="min-w-0 flex-1 truncate text-body" translate="no">
                    {name}
                  </span>
                  <span className="shrink-0 text-meta text-muted-foreground">{c ? seenText(c) : on ? "On the list" : ""}</span>
                </label>
              </li>
            );
          })}
        </ul>
        <div aria-live="polite">
          {loading && (
            <p className="flex items-center gap-2 text-meta text-muted-foreground">
              <Spinner {...SPINNER} /> Looking for open and recent apps…
            </p>
          )}
          {error && (
            <div className="flex flex-wrap items-center gap-2">
              <FieldError>{`Couldn’t list the apps on this PC: ${error}`}</FieldError>
              <Button variant="outline" size="sm" onClick={() => void load()}>
                Try again
              </Button>
            </div>
          )}
        </div>
      </div>
      <form
        className="flex flex-col gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (!draft.trim()) return;
          set("blockedApps", addPrivateApps(s.blockedApps, draft));
          setShown((cur) => addPrivateApps(cur, draft));
          setDraft("");
        }}
      >
        <label htmlFor={`${id}-add`} className="text-meta font-medium">
          Add another app by name
        </label>
        <div className="flex flex-wrap gap-2">
          <Input
            id={`${id}-add`}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            placeholder="Banking app, for example"
            autoComplete="off"
            spellCheck={false}
            className="min-w-0 flex-1 basis-56"
          />
          <Button type="submit" variant="secondary" disabled={!draft.trim()}>
            <Plus aria-hidden /> Add
          </Button>
        </div>
      </form>
    </>
  );
}

function StartStep({ s, set, keyMissing, hasKey }: { s: Settings; set: SetFn; keyMissing: boolean; hasKey: boolean }) {
  const id = React.useId();
  const p = providerInfo(s.provider);
  const apps = s.blockedApps.filter(Boolean);
  const kept = s.retentionDays === 1 ? "1 day" : `${s.retentionDays} days`;
  const rows: [string, string][] = [
    ["Provider", `${p.title}, ${s.model}${p.key === "none" ? "" : hasKey ? ", key saved" : p.key === "required" ? ", no key yet" : ""}`],
    ["Screenshots", `Every ${s.captureIntervalSecs} s, kept ${kept}`],
    ["Private apps", apps.length === 0 ? "None" : apps.length <= 4 ? apps.join(", ") : `${apps.slice(0, 3).join(", ")} and ${apps.length - 3} more`],
  ];
  return (
    <>
      {keyMissing && (
        <Banner tone="warn" icon={<TriangleAlert aria-hidden />} title="No API key is saved yet">
          Daytrace records, but the screenshots wait until you add a key under Settings, Model provider.
        </Banner>
      )}
      <dl className="flex flex-col divide-y rounded-lg border text-body">
        {rows.map(([k, v]) => (
          <div key={k} className="flex flex-wrap gap-x-6 gap-y-0.5 px-4 py-2.5">
            <dt className="w-32 text-muted-foreground">{k}</dt>
            <dd className="min-w-0 flex-1" translate={k === "Private apps" ? "no" : undefined}>
              {v}
            </dd>
          </div>
        ))}
      </dl>
      <div className="flex items-center justify-between gap-4 rounded-lg border px-4 py-3">
        <label htmlFor={`${id}-login`} className="flex flex-col gap-0.5">
          <span className="text-body font-medium">Start Daytrace when I sign in to Windows</span>
          <span className="text-meta text-muted-foreground">Daytrace starts in the tray.</span>
        </label>
        <Switch id={`${id}-login`} checked={s.launchAtLogin} onCheckedChange={(v) => set("launchAtLogin", v)} />
      </div>
      <p className="flex items-start gap-2 text-meta text-muted-foreground">
        <MonitorUp className="mt-0.5 size-4 shrink-0" aria-hidden />
        Closing the window keeps Daytrace running in the tray. Pause or quit it from the tray icon at any time.
      </p>
    </>
  );
}
