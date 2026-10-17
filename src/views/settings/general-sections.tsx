import * as React from "react";
import { listen } from "@tauri-apps/api/event";
import { BellRing, ExternalLink, KeyRound, Laptop, Moon, RefreshCw, Sun, type LucideIcon } from "lucide-react";
import type { NotificationPrefs } from "@/api";
import { useAppState } from "@/app/state";
import { Button } from "@/components/ui/button";
import { RadioGroup, RadioGroupItem } from "@/components/ui/radio-group";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import type { Appearance } from "@/lib/theme";
import { errorText } from "@/lib/utils";
import { OPEN_ONBOARDING_EVENT, platformApi, type UpdateStatus } from "@/views/platform-api";
import { FieldError, Group, OutcomeLine, Row, type Outcome } from "./fields";
import { useSave } from "./save";
import { AutoInstallRows } from "./update-install";
import { DebugInfoGroup } from "./debug-info";
import mitLicence from "../../../LICENSE?raw";
import oflLicence from "@/assets/cascadia-mono-OFL.txt?raw";

const SPINNER = { "aria-hidden": true, role: undefined, "aria-label": undefined } as const;

const NOTIFICATIONS: { key: keyof NotificationPrefs; label: string; description?: string }[] = [
  { key: "recapReady", label: "The daily recap is ready" },
  { key: "firstCards", label: "The first timeline cards after setup are ready" },
  { key: "recordingProblems", label: "Daytrace can’t capture the screen" },
  { key: "analysisProblems", label: "Cards can’t be made", description: "A missing or refused API key, no credits, a local model that is offline, or repeated failures." },
  { key: "missingApiKey", label: "Screenshots are waiting for an API key" },
  { key: "updates", label: "A new version is available" },
];

/** Settings > Notifications: one switch per kind, and a test notification. */
export function NotificationsSection() {
  const app = useAppState();
  const prefs = app.settings.settings.notifications;
  const save = useSave();
  const id = React.useId();
  const [error, setError] = React.useState<string | null>(null);
  const [test, setTest] = React.useState<Outcome | null>(null);
  const [sending, setSending] = React.useState(false);

  async function sendTest() {
    setSending(true);
    setTest(null);
    try {
      await platformApi.testNotification();
      setTest({ ok: true, text: "Sent. If nothing appears, check Notifications and Do not disturb in Windows Settings > System." });
    } catch (e) {
      setTest({ ok: false, text: errorText(e) });
    } finally {
      setSending(false);
    }
  }

  return (
    <>
      <Group title="Windows notifications" description="Each kind is shown at most once a day, and not while the Daytrace window is in front.">
        {NOTIFICATIONS.map((n) => (
          <Row key={n.key} label={n.label} description={n.description} htmlFor={`${id}-${n.key}`}>
            <Switch
              id={`${id}-${n.key}`}
              checked={prefs[n.key]}
              onCheckedChange={(v) => {
                setError(null);
                save((cur) => ({ notifications: { ...cur.notifications, [n.key]: v } })).catch((e) => setError(`Couldn’t save: ${errorText(e)}`));
              }}
            />
          </Row>
        ))}
        {error && (
          <div className="px-4 py-3">
            <FieldError>{error}</FieldError>
          </div>
        )}
      </Group>
      <Group title="Test">
        <Row label="Check that notifications get through" description="Shows one notification now. Windows can hide them under Do not disturb or per-app settings.">
          <Button variant="outline" onClick={() => void sendTest()} disabled={sending}>
            {sending ? <Spinner {...SPINNER} /> : <BellRing aria-hidden />}
            {sending ? "Sending…" : "Send a test notification"}
          </Button>
        </Row>
        <div aria-live="polite" className="empty:hidden">
          {test && <OutcomeLine outcome={test} className="mx-4 my-3" />}
        </div>
      </Group>
    </>
  );
}

const THEMES: { value: Appearance; label: string; note: string; icon: LucideIcon }[] = [
  { value: "system", label: "Same as Windows", note: "Light or dark, as set in Windows Settings > Personalization > Colors.", icon: Laptop },
  { value: "light", label: "Light", note: "", icon: Sun },
  { value: "dark", label: "Dark", note: "", icon: Moon },
];

/** Settings > Appearance: the theme (the same setting as the switch at the bottom of the rail). */
export function AppearanceSection() {
  const app = useAppState();
  const choice = app.settings.settings.appearance;
  const [error, setError] = React.useState<string | null>(null);
  return (
    <Group title="Theme" description="The theme switch at the bottom of the side bar changes the same setting.">
      <RadioGroup
        value={choice}
        onValueChange={(v) => {
          setError(null);
          app.saveAppearance(v as Appearance).catch((e) => setError(`Couldn’t save the theme: ${errorText(e)}`));
        }}
        aria-label="Theme"
        className="gap-0 divide-y"
      >
        {THEMES.map((t) => (
          <label key={t.value} className="flex items-start gap-3 px-4 py-3 hover:bg-surface-2/60">
            <RadioGroupItem value={t.value} className="mt-0.5" aria-label={t.label} />
            <t.icon className="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden />
            <span className="min-w-0">
              <span className="block text-body font-medium">{t.label}</span>
              {t.note && <span className="block text-meta text-muted-foreground">{t.note}</span>}
            </span>
          </label>
        ))}
      </RadioGroup>
      {error && (
        <div className="px-4 py-3">
          <FieldError>{error}</FieldError>
        </div>
      )}
    </Group>
  );
}

function updateText(u: UpdateStatus): Outcome & { tone: "ok" | "bad" | "muted" } {
  // A failed check keeps the release found before.
  if (u.available && u.latestVersion)
    return { ok: true, tone: "ok", text: `Version ${u.latestVersion} is available.${u.error ? ` The last check failed: ${u.error}` : ""}` };
  if (u.error) return { ok: false, tone: "bad", text: u.error };
  if (u.noReleases) return { ok: true, tone: "muted", text: "No release has been published yet." };
  if (u.checkedAt) return { ok: true, tone: "muted", text: "Daytrace is up to date." };
  return { ok: true, tone: "muted", text: "Not checked yet." };
}

/** Settings > Updates and about: version, update check, the first-run setup, what Daytrace is. */
export function AboutSection() {
  const app = useAppState();
  const s = app.settings.settings;
  const save = useSave();
  const id = React.useId();
  const [update, setUpdate] = React.useState<UpdateStatus | null>(null);
  const [version, setVersion] = React.useState<string | null>(null);
  const [checking, setChecking] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    platformApi
      .info()
      .then((i) => {
        setUpdate(i.update);
        setVersion(i.version);
      })
      .catch(() => {});
    const un = listen<UpdateStatus>("platform-update", (e) => setUpdate(e.payload));
    return () => void un.then((f) => f());
  }, []);

  async function checkNow() {
    setChecking(true);
    try {
      setUpdate(await platformApi.checkUpdates());
    } catch (e) {
      setUpdate((u) => (u ? { ...u, error: errorText(e) } : u));
    } finally {
      setChecking(false);
    }
  }

  const u = update ? updateText(update) : null;
  const current = update?.currentVersion ?? version;

  return (
    <>
      <Group title="Updates" description="Daytrace looks for a new release on GitHub.">
        <Row label={current ? `Daytrace ${current}` : "Daytrace"} description={<span aria-live="polite">{u ? u.text : "Reading the version…"}</span>}>
          <Button variant="outline" onClick={() => void checkNow()} disabled={checking}>
            {checking ? <Spinner {...SPINNER} /> : <RefreshCw aria-hidden />}
            {checking ? "Checking…" : "Check now"}
          </Button>
          {update?.available && (
            <Button onClick={() => platformApi.openRelease().catch((e) => setError(errorText(e)))}>
              <ExternalLink aria-hidden /> Open the release page
            </Button>
          )}
        </Row>
        <Row label="Check for updates once a day" htmlFor={`${id}-check`}>
          <Switch
            id={`${id}-check`}
            checked={s.checkUpdates}
            onCheckedChange={(v) => {
              setError(null);
              save({ checkUpdates: v }).catch((e) => setError(`Couldn’t save: ${errorText(e)}`));
            }}
          />
        </Row>
        <AutoInstallRows />
        {error && (
          <div className="px-4 py-3">
            <FieldError>{error}</FieldError>
          </div>
        )}
      </Group>

      <Group title="Setup">
        <Row label="Run setup again" description="Go through the first-run steps again: what Daytrace sends where, the model, private apps and starting with Windows.">
          <Button variant="outline" onClick={() => window.dispatchEvent(new CustomEvent(OPEN_ONBOARDING_EVENT))}>
            <KeyRound aria-hidden /> Open setup
          </Button>
        </Row>
      </Group>

      <DebugInfoGroup />

      <Group title="About">
        <Row
          label="Daytrace is an unofficial Windows port of Dayflow"
          description={
            <>
              <p>
                The analysis design, its constants and the prompt texts come from Dayflow by Jerry Liu (© 2025, MIT License). The app around
                them is a new implementation. Not affiliated with or endorsed by the Dayflow project.
              </p>
              <p>Daytrace is open source under the MIT License.</p>
              <LicenceText label="MIT License text" text={mitLicence} />
            </>
          }
        />
        <Row
          label="Daytrace Mono"
          description={
            <>
              <p>
                Clock times use a subset of Cascadia Mono, © 2019 – present Microsoft Corporation, with Reserved Font Name Cascadia Code,
                licensed under the SIL Open Font License 1.1. As the licence asks for a modified font, the subset carries its own name,
                Daytrace Mono. A Cascadia Mono installed on this PC is used instead when present.
              </p>
              <LicenceText label="SIL Open Font License text" text={oflLicence} />
            </>
          }
        />
      </Group>
    </>
  );
}

/** A licence text, collapsed until asked for. */
function LicenceText({ label, text }: { label: string; text: string }) {
  return (
    <details className="mt-1.5 text-caption">
      <summary className="w-fit cursor-default rounded-sm select-none hover:text-foreground">{label}</summary>
      <pre className="scroll-thin mt-1.5 max-h-64 overflow-auto rounded-md border bg-surface-2 p-3 font-mono text-caption whitespace-pre-wrap text-foreground [overflow-wrap:anywhere]">
        {text.trim()}
      </pre>
    </details>
  );
}
