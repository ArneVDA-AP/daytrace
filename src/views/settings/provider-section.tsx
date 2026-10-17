import * as React from "react";
import { Lock, Undo2 } from "lucide-react";
import { api, type ProviderKind, type Settings } from "@/api";
import { useAppState } from "@/app/state";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { cn, errorText } from "@/lib/utils";
import { CliPathField } from "./cli-path-field";
import { Choice, FieldError, Group, OutcomeLine, Row, type Outcome } from "./fields";
import { cliCommand, cliCompany, cliProduct, cliSignIn, defaultPipeline, keySource, PROVIDERS, providerInfo, type ProviderInfo } from "./providers";
import { savedSettings, useCommitField, useSave } from "./save";

/** Settings > Model provider: the main provider, its connection and analysis options, the fallback. */
export function ProviderSection() {
  return (
    <>
      <MainProvider />
      <FallbackProvider />
    </>
  );
}

/** "Blue (812 ms)" from test_provider, in words. */
export function testText(out: string, model: string): string {
  const m = out.trim().match(/^([\s\S]*?)\s*\((\d+) ms\)$/);
  if (!m) return `The model answered: ${out.trim()}`;
  const secs = (Number(m[2]) / 1000).toLocaleString("en-US", { maximumFractionDigits: 1, minimumFractionDigits: 1 });
  return `Connected. ${model} answered “${m[1].replace(/[.\s]+$/, "")}” in ${secs} s.`;
}

const SPINNER = { "aria-hidden": true, role: undefined, "aria-label": undefined } as const;

type Saved = Pick<Settings, "provider" | "baseUrl" | "model" | "pipeline">;

function MainProvider() {
  const app = useAppState();
  const s = app.settings.settings;
  const hasKey = app.settings.hasApiKey;
  const save = useSave();
  const [choice, setChoice] = React.useState<ProviderKind | null>(null);
  const [previous, setPrevious] = React.useState<Saved | null>(null);
  const [providerError, setProviderError] = React.useState<string | null>(null);
  const [key, setKey] = React.useState("");
  const [keyOutcome, setKeyOutcome] = React.useState<Outcome | null>(null);
  const [models, setModels] = React.useState<string[] | null>(null);
  const [modelsError, setModelsError] = React.useState<string | null>(null);
  const [test, setTest] = React.useState<Outcome | null>(null);
  const [busy, setBusy] = React.useState<"models" | "test" | "key" | null>(null);
  const seq = React.useRef(0);

  const current = choice ?? s.provider;
  const info = providerInfo(current);
  const ids = useIds();

  // A key typed but not saved when leaving Settings is saved then, for the provider it was typed
  // for (a provider change clears it). Queued after the field saves of the same moment.
  const keyRef = React.useRef(key);
  keyRef.current = key;
  React.useEffect(
    () => () => {
      const k = keyRef.current.trim();
      if (k) queueMicrotask(() => void save({}, { apiKey: k }).catch(() => undefined));
    },
    [save],
  );

  function forget() {
    // A key typed for the previous provider is not saved under the new one.
    setKey("");
    setKeyOutcome(null);
    setModels(null);
    setModelsError(null);
    setTest(null);
    setProviderError(null);
  }

  async function changeProvider(id: ProviderKind) {
    if (id === current) return;
    const from: Saved = previous ?? { provider: s.provider, baseUrl: s.baseUrl, model: s.model, pipeline: s.pipeline };
    const n = ++seq.current;
    setChoice(id);
    forget();
    try {
      const [baseUrl, model] = await api.providerDefaults(id);
      // Arrowing through the list: only the last choice is saved.
      if (n !== seq.current) return;
      await save({ provider: id, baseUrl, model, pipeline: defaultPipeline(id) });
      setPrevious(from.provider === id ? null : from);
    } catch (e) {
      if (n === seq.current) setProviderError(errorText(e));
    } finally {
      if (n === seq.current) setChoice(null);
    }
  }

  async function switchBack() {
    if (!previous) return;
    forget();
    try {
      await save(previous);
      setPrevious(null);
    } catch (e) {
      setProviderError(errorText(e));
    }
  }

  const baseUrl = useCommitField(s.baseUrl, (v) => save({ baseUrl: v.trim() }));
  const model = useCommitField(s.model, (v) => save({ model: v.trim() }));
  const cliPath = useCommitField(s.baseUrl, (v) => save({ baseUrl: v.trim() }));

  async function loadModels() {
    setBusy("models");
    setModelsError(null);
    try {
      const cur = await savedSettings();
      const list = await api.listModels(cur.provider, cur.baseUrl, key);
      setModels(list);
    } catch (e) {
      setModels(null);
      setModelsError(`Couldn’t list models: ${errorText(e)}`);
    } finally {
      setBusy(null);
    }
  }

  async function saveKey() {
    const k = key.trim();
    if (!k) return;
    setBusy("key");
    setKeyOutcome(null);
    try {
      await save({}, { apiKey: k });
      setKey("");
      setKeyOutcome({ ok: true, text: "Key saved in Windows Credential Manager." });
    } catch (e) {
      setKeyOutcome({ ok: false, text: `Couldn’t save the key: ${errorText(e)}` });
    } finally {
      setBusy(null);
    }
  }

  async function runTest() {
    setBusy("test");
    setTest(null);
    try {
      // After the saves started by leaving a field, so the test uses what is on screen.
      const cur = await savedSettings();
      const out = await api.testProvider(cur.provider, cur.baseUrl, cur.model, key);
      setTest({ ok: true, text: testText(out, cur.model) });
    } catch (e) {
      setTest({ ok: false, text: errorText(e) });
    } finally {
      setBusy(null);
    }
  }

  const [tokensIn, tokensOut] = app.status.tokensToday;
  const keyPlaceholder = hasKey ? "Saved. Type a new key to replace it." : info.key === "optional" ? "Optional" : "Paste your API key";
  const modelHelp = models
    ? models.length === 0
      ? "The provider listed no models. Type the model name."
      : `${models.length} ${models.length === 1 ? "model" : "models"} found. Type in the field to pick one.`
    : null;

  const serverRow = (
    <Row
      label="Server address"
      htmlFor={ids.url}
      stack
      description={
        current === "openai_compatible"
          ? "The address of the server’s OpenAI API, usually ending in /v1. The key below belongs to this server."
          : current === "anthropic"
            ? "Change it only when you send requests through another server."
            : undefined
      }
    >
      <Input id={ids.url} {...baseUrl.bind} spellCheck={false} autoComplete="off" className="font-mono text-meta" />
      <FieldError>{baseUrl.error}</FieldError>
    </Row>
  );

  return (
    <>
      <Group title="Provider" description="Who turns your screenshots into timeline cards. Every 15 minutes Daytrace sends that batch of screenshots to this provider.">
        <RadioGroup value={current} onValueChange={(v) => void changeProvider(v as ProviderKind)} aria-label="Model provider" className="gap-0 divide-y">
          {PROVIDERS.map((p) => (
            <ProviderOption key={p.id} p={p} selected={current === p.id} />
          ))}
        </RadioGroup>
        {(previous || providerError) && (
          <div className="flex flex-col gap-2 px-4 py-3" aria-live="polite">
            {previous && (
              <p className="flex flex-wrap items-center gap-x-3 gap-y-1 text-meta text-muted-foreground">
                <span>
                  Switched from {providerInfo(previous.provider).title} ({previous.model}). The model and server were reset to {info.title}’s defaults.
                </span>
                <Button variant="outline" size="sm" onClick={switchBack}>
                  <Undo2 aria-hidden /> Switch back
                </Button>
              </p>
            )}
            <FieldError>{providerError && `Couldn’t change the provider: ${providerError}`}</FieldError>
          </div>
        )}
      </Group>

      <Group
        title={info.title}
        description={
          info.cli
            ? `Every 15 minutes Daytrace runs ${cliCommand(current)} on this PC with that batch’s screenshots. It uses the account you signed in with in the CLI, so the screenshots go to ${cliCompany(current)} under that account. They are copied to a temporary folder for the CLI and deleted right after.`
            : info.local
              ? "The model runs on this PC: screenshots never leave it."
              : `Every 15 minutes a sample of that period’s screenshots is sent to ${info.id === "anthropic" ? "Anthropic" : "this server"} to write the cards.`
        }
      >
        {info.cli ? (
          <Row
            label={`Path to ${cliCommand(current)}`}
            htmlFor={ids.cli}
            stack
            description={
              <p id={ids.cliHelp}>
                Install {cliProduct(current)}, then sign in once: {cliSignIn(current)}. Leave the path empty unless Daytrace can’t find it.
                {current === "claude_cli" ? " On Windows, Claude Code also needs Git for Windows." : ""}
              </p>
            }
          >
            <CliPathField id={ids.cli} provider={current} input={cliPath.bind} describedBy={ids.cliHelp} />
            <FieldError>{cliPath.error}</FieldError>
          </Row>
        ) : (
          current !== "anthropic" && serverRow
        )}

        {info.key !== "none" && (
          <Row
            label={info.key === "optional" ? "API key (optional)" : "API key"}
            htmlFor={ids.key}
            stack
            description={
              <>
                <p className="inline-flex items-center gap-1.5">
                  <Lock className="size-3.5 shrink-0" aria-hidden /> Stored in Windows Credential Manager, not in the settings file.
                </p>
                {keySource(current) && <p>{keySource(current)}</p>}
              </>
            }
          >
            <form
              className="flex w-full flex-wrap gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                void saveKey();
              }}
            >
              <Input
                id={ids.key}
                type="password"
                value={key}
                placeholder={keyPlaceholder}
                autoComplete="off"
                spellCheck={false}
                aria-describedby={ids.keyState}
                onChange={(e) => {
                  setKey(e.target.value);
                  setKeyOutcome(null);
                }}
                className="min-w-0 flex-1 basis-64 font-mono text-meta placeholder:font-sans"
              />
              <Button type="submit" variant="outline" disabled={busy !== null || !key.trim()}>
                {busy === "key" && <Spinner {...SPINNER} />}
                {busy === "key" ? "Saving…" : "Save key"}
              </Button>
            </form>
            <div id={ids.keyState} aria-live="polite" className="flex flex-col gap-1">
              {keyOutcome ? (
                <OutcomeLine outcome={keyOutcome} />
              ) : key.trim() ? (
                <p className="text-meta text-warn-foreground">Not saved yet. Select Save key, or Test connection to try it first.</p>
              ) : (
                <p className="text-meta text-muted-foreground">
                  {hasKey ? "A key is saved for this provider." : info.key === "required" ? "No key saved yet: screenshots wait until you add one." : "No key saved."}
                </p>
              )}
            </div>
          </Row>
        )}

        <Row label="Model" htmlFor={ids.model} stack>
          <div className="flex w-full flex-wrap gap-2">
            <Input
              id={ids.model}
              {...model.bind}
              list={ids.modelList}
              spellCheck={false}
              autoComplete="off"
              aria-describedby={ids.modelHelp}
              className="min-w-0 flex-1 basis-64 font-mono text-meta"
            />
            <datalist id={ids.modelList}>{models?.map((m) => <option key={m} value={m} />)}</datalist>
            <Button type="button" variant="outline" onClick={loadModels} disabled={busy !== null}>
              {busy === "models" && <Spinner {...SPINNER} />}
              {busy === "models" ? "Loading…" : info.cli ? "Show suggested models" : "Show available models"}
            </Button>
          </div>
          <div id={ids.modelHelp} aria-live="polite" className="flex flex-col gap-1">
            {modelHelp && <p className="text-meta text-muted-foreground">{modelHelp}</p>}
            <FieldError>{model.error ?? modelsError}</FieldError>
          </div>
        </Row>

        {current === "anthropic" && serverRow}


        {info.cli ? (
          <Row
            label="Analysis"
            description="The CLI reads the screenshots and writes the cards in one step. If that step fails, Daytrace tries again in the Detailed mode with the same CLI, or with the fallback provider below when the CLI is signed out, out of usage or does not answer within five minutes."
          />
        ) : (
          <Row label="Analysis" labelId={ids.pipeline} stack>
            <RadioGroup
              value={s.pipeline}
              onValueChange={(v) => void save({ pipeline: v as Settings["pipeline"] }).catch(() => undefined)}
              aria-labelledby={ids.pipeline}
              className="gap-2"
            >
              <PipelineOption
                value="cloud"
                title="Detailed"
                text="One request with 15 screenshots, then the last 45 minutes are rewritten as a whole. Best with Claude or other large models."
              />
              <PipelineOption
                value="local"
                title="Lightweight"
                text="Each screenshot is described on its own, then combined into one card. Suited to small local vision models such as qwen3-vl:4b."
              />
            </RadioGroup>
          </Row>
        )}

        {current === "anthropic" && (
          <>
            <Row label="Model reasoning" htmlFor={ids.reasoning} description="How much the model thinks before it writes a card.">
              <Choice
                id={ids.reasoning}
                value={s.reasoning}
                className="w-80 max-w-full"
                onChange={(v) => void save({ reasoning: v }).catch(() => undefined)}
                options={[
                  { value: "low", label: "Low (recommended)" },
                  { value: "off", label: "Off (cheapest, breaks Dayflow’s card rules more often)" },
                  { value: "default", label: "Model default (about 1.5× the output cost of Low)" },
                ]}
              />
            </Row>
            <Row
              label="Use the Batch API"
              htmlFor={ids.batch}
              description="Half the cost, but cards can take from a few minutes up to an hour to appear."
            >
              <Switch id={ids.batch} checked={s.useBatchApi} onCheckedChange={(v) => void save({ useBatchApi: v }).catch(() => undefined)} />
            </Row>
          </>
        )}

        <Row
          label="Test connection"
          stack
          description={`Sends one small image to the model and waits for its answer. Usage today: ${tokensIn.toLocaleString("en-US")} input and ${tokensOut.toLocaleString("en-US")} output tokens.`}
        >
          <div className="flex flex-wrap items-center gap-3">
            <Button type="button" variant="secondary" onClick={runTest} disabled={busy !== null}>
              {busy === "test" && <Spinner {...SPINNER} />}
              {busy === "test" ? "Testing…" : key.trim() ? "Test with the typed key" : "Test connection"}
            </Button>
            {busy === "test" && info.cli && <span className="text-meta text-muted-foreground">The CLI can take up to a minute.</span>}
          </div>
          <div aria-live="polite">
            <OutcomeLine outcome={test} />
          </div>
        </Row>
      </Group>
    </>
  );
}

function useIds() {
  const base = React.useId();
  return React.useMemo(
    () => ({
      cli: `${base}-cli`,
      cliHelp: `${base}-cli-help`,
      url: `${base}-url`,
      model: `${base}-model`,
      modelList: `${base}-models`,
      modelHelp: `${base}-model-help`,
      key: `${base}-key`,
      keyState: `${base}-key-state`,
      pipeline: `${base}-pipeline`,
      reasoning: `${base}-reasoning`,
      batch: `${base}-batch`,
    }),
    [base],
  );
}

/** One provider in the radio list: name, "Recommended", and what it costs and sends where. */
export function ProviderOption({ p, selected, card }: { p: ProviderInfo; selected: boolean; /** Setup choice card, with the short note. */ card?: boolean }) {
  const id = React.useId();
  return (
    <label
      className={cn(
        "flex items-start gap-3 px-4 py-3 hover:bg-surface-2/60",
        card && "rounded-lg border px-3.5 py-3",
        card && selected && "border-foreground/60 bg-surface-2/60",
      )}
    >
      <RadioGroupItem value={p.id} className="mt-0.5" aria-labelledby={`${id}-t`} aria-describedby={`${id}-d`} />
      <span className="min-w-0 flex-1">
        <span id={`${id}-t`} className="block text-body font-medium" translate="no">
          {p.title}
        </span>
        <span id={`${id}-d`} className="block text-meta text-muted-foreground">
          {card ? p.brief : p.note}
        </span>
      </span>
      {p.id === "anthropic" && <span className="shrink-0 rounded-sm bg-surface-2 px-1.5 py-0.5 text-caption text-muted-foreground">Recommended</span>}
    </label>
  );
}

function PipelineOption({ value, title, text }: { value: string; title: string; text: string }) {
  const id = React.useId();
  return (
    <label className="flex items-start gap-3 rounded-md py-1">
      <RadioGroupItem value={value} className="mt-0.5" aria-labelledby={`${id}-t`} aria-describedby={`${id}-d`} />
      <span className="min-w-0">
        <span id={`${id}-t`} className="block text-body font-medium">
          {title}
        </span>
        <span id={`${id}-d`} className="block text-meta text-muted-foreground">
          {text}
        </span>
      </span>
    </label>
  );
}

// ---- fallback provider (Dayflow's secondary provider) ----

interface Fallback {
  kind: ProviderKind;
  baseUrl: string;
  model: string;
}

const trimUrl = (u: string) => u.trim().replace(/\/+$/, "");

function FallbackProvider() {
  const app = useAppState();
  const s = app.settings.settings;
  const save = useSave();
  const ids = useIds();
  /** A choice equal to the main provider: the backend drops such a fallback, so it waits here. */
  const [pending, setPending] = React.useState<Fallback | null>(null);
  const [key, setKey] = React.useState("");
  const [hasKey, setHasKey] = React.useState<boolean | null>(null);
  const [outcome, setOutcome] = React.useState<Outcome | null>(null);
  const [busy, setBusy] = React.useState<"key" | "test" | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [dropped, setDropped] = React.useState(false);

  const saved: Fallback | null = s.fallbackProvider ? { kind: s.fallbackProvider, baseUrl: s.fallbackBaseUrl, model: s.fallbackModel } : null;
  const fb = pending ?? saved;
  const info = fb ? providerInfo(fb.kind) : null;
  const same = (f: Fallback) => f.kind === s.provider && trimUrl(f.baseUrl) === trimUrl(s.baseUrl) && f.model.trim() === s.model.trim();

  // The backend removes a fallback that became equal to the main provider (a change above).
  const prev = React.useRef(s.fallbackProvider);
  const chosenNone = React.useRef(false);
  React.useEffect(() => {
    if (prev.current && !s.fallbackProvider && !chosenNone.current) setDropped(true);
    if (s.fallbackProvider) setDropped(false);
    chosenNone.current = false;
    prev.current = s.fallbackProvider;
  }, [s.fallbackProvider]);

  // The key slot depends on the provider and, for generic endpoints, on the server.
  const slotKind = saved?.kind;
  const slotUrl = saved?.baseUrl ?? "";
  const slotInfo = slotKind ? providerInfo(slotKind) : null;
  React.useEffect(() => {
    let live = true;
    setHasKey(null);
    if (slotKind && slotInfo?.key !== "none") {
      api
        .providerKeyStatus(slotKind, slotUrl)
        .then((v) => live && setHasKey(v))
        .catch(() => live && setHasKey(false));
    }
    return () => {
      live = false;
    };
  }, [slotKind, slotUrl, slotInfo?.key]);

  // A key typed but not saved when leaving is saved then (only for a saved fallback).
  const keyRef = React.useRef({ key, saved });
  keyRef.current = { key, saved };
  React.useEffect(
    () => () => {
      const { key: k, saved: f } = keyRef.current;
      if (k.trim() && f && providerInfo(f.kind).key !== "none") {
        queueMicrotask(() => void api.providerSaveKey(f.kind, f.baseUrl, k.trim()).catch(() => undefined));
      }
    },
    [],
  );

  async function store(next: Fallback) {
    if (same(next)) {
      setPending(next);
      return;
    }
    await save({ fallbackProvider: next.kind, fallbackBaseUrl: next.baseUrl.trim(), fallbackModel: next.model.trim() });
    setPending(null);
  }

  async function choose(value: string) {
    setOutcome(null);
    setError(null);
    setKey("");
    try {
      if (value === "") {
        setPending(null);
        chosenNone.current = true;
        await save({ fallbackProvider: null });
        return;
      }
      const kind = value as ProviderKind;
      const [baseUrl, model] = await api.providerDefaults(kind);
      await store({ kind, baseUrl, model });
    } catch (e) {
      setError(errorText(e));
    }
  }

  const sameMessage = "That is the same provider, server and model as the main one, so Daytrace would skip it. Use another model or server.";
  const url = useCommitField(fb?.baseUrl ?? "", (v) => store({ ...fb!, baseUrl: v }), {
    validate: (v) => (fb && same({ ...fb, baseUrl: v }) && !pending ? sameMessage : null),
  });
  const model = useCommitField(fb?.model ?? "", (v) => store({ ...fb!, model: v }), {
    validate: (v) => (fb && same({ ...fb, model: v }) && !pending ? sameMessage : null),
  });

  async function saveKey() {
    if (!saved || !key.trim()) return;
    setBusy("key");
    setOutcome(null);
    try {
      await api.providerSaveKey(saved.kind, saved.baseUrl, key.trim());
      setKey("");
      setHasKey(true);
      setOutcome({ ok: true, text: "Key saved in Windows Credential Manager." });
    } catch (e) {
      setOutcome({ ok: false, text: `Couldn’t save the key: ${errorText(e)}` });
    } finally {
      setBusy(null);
    }
  }

  async function test() {
    if (!fb) return;
    setBusy("test");
    setOutcome(null);
    try {
      const cur = await savedSettings();
      const f = pending ?? (cur.fallbackProvider ? { kind: cur.fallbackProvider, baseUrl: cur.fallbackBaseUrl, model: cur.fallbackModel } : fb);
      const out = await api.testProvider(f.kind, f.baseUrl, f.model, key || undefined, true);
      setOutcome({ ok: true, text: testText(out, f.model) });
    } catch (e) {
      setOutcome({ ok: false, text: errorText(e) });
    } finally {
      setBusy(null);
    }
  }

  return (
    <Group
      title="Fallback provider"
      description="When the main provider fails a step (for example because it is offline, signed out or out of usage), Daytrace runs that step once on this provider and uses it for the rest of that batch. It uses the Detailed mode, or Lightweight for Ollama and LM Studio."
    >
      <Row label="Fallback" htmlFor={ids.cli + "-fb"}>
        <Choice<string>
          id={ids.cli + "-fb"}
          value={fb?.kind ?? ""}
          className="w-80 max-w-full"
          onChange={(v) => void choose(v)}
          options={[{ value: "", label: "None" }, ...PROVIDERS.map((p) => ({ value: p.id as string, label: p.label }))]}
        />
      </Row>
      {(dropped || error) && !fb && (
        <div className="px-4 py-3" aria-live="polite">
          {dropped && <p className="text-meta text-muted-foreground">The fallback was removed because it became the same as the main provider.</p>}
          <FieldError>{error}</FieldError>
        </div>
      )}
      {fb && info && (
        <>
          {pending && (
            <div className="px-4 py-3" role="status">
              <p className="rounded-md border border-warn/50 bg-warn-surface px-3 py-2 text-meta">
                This is the same provider and model as the main one, so it isn’t saved. Change the model or the server address to use it as the fallback.
              </p>
            </div>
          )}
          {info.cli ? (
            <Row label={`Path to ${cliCommand(fb.kind)}`} htmlFor={ids.cli + "-fbp"} stack description={`Sign in once: ${cliSignIn(fb.kind)}. Leave the path empty unless Daytrace can’t find it.`}>
              <CliPathField id={ids.cli + "-fbp"} provider={fb.kind} input={url.bind} />
              <FieldError>{url.error}</FieldError>
            </Row>
          ) : (
            <Row label="Server address" htmlFor={ids.url + "-fb"} stack>
              <Input id={ids.url + "-fb"} {...url.bind} spellCheck={false} autoComplete="off" className="font-mono text-meta" />
              <FieldError>{url.error}</FieldError>
            </Row>
          )}
          <Row label="Model" htmlFor={ids.model + "-fb"} stack>
            <Input id={ids.model + "-fb"} {...model.bind} spellCheck={false} autoComplete="off" className="font-mono text-meta" />
            <FieldError>{model.error}</FieldError>
          </Row>
          {info.key !== "none" && (
            <Row
              label={info.key === "optional" ? "API key (optional)" : "API key"}
              htmlFor={ids.key + "-fb"}
              stack
              description={
                <p className="inline-flex items-center gap-1.5">
                  <Lock className="size-3.5 shrink-0" aria-hidden /> Its own key, stored in Windows Credential Manager.
                </p>
              }
            >
              <form
                className="flex w-full flex-wrap gap-2"
                onSubmit={(e) => {
                  e.preventDefault();
                  void saveKey();
                }}
              >
                <Input
                  id={ids.key + "-fb"}
                  type="password"
                  value={key}
                  autoComplete="off"
                  spellCheck={false}
                  disabled={!saved}
                  aria-describedby={ids.keyState + "-fb"}
                  placeholder={hasKey === null ? "Checking…" : hasKey ? "Saved. Type a new key to replace it." : info.key === "optional" ? "Optional" : "Paste the API key"}
                  onChange={(e) => {
                    setKey(e.target.value);
                    setOutcome(null);
                  }}
                  className="min-w-0 flex-1 basis-64 font-mono text-meta placeholder:font-sans"
                />
                <Button type="submit" variant="outline" disabled={busy !== null || !key.trim() || !saved}>
                  {busy === "key" && <Spinner {...SPINNER} />}
                  {busy === "key" ? "Saving…" : "Save key"}
                </Button>
              </form>
              <p id={ids.keyState + "-fb"} className="text-meta text-muted-foreground">
                {key.trim()
                  ? "Not saved yet. Select Save key."
                  : info.key === "required" && hasKey === false
                    ? "Without a key this fallback is skipped."
                    : hasKey
                      ? "A key is saved for this fallback."
                      : ""}
              </p>
            </Row>
          )}
          <Row label="Test fallback" stack>
            <div>
              <Button type="button" variant="secondary" onClick={test} disabled={busy !== null}>
                {busy === "test" && <Spinner {...SPINNER} />}
                {busy === "test" ? "Testing…" : "Test fallback"}
              </Button>
            </div>
            <div aria-live="polite">
              <OutcomeLine outcome={outcome} />
            </div>
          </Row>
        </>
      )}
    </Group>
  );
}
