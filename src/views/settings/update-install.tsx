import * as React from "react";
import { listen } from "@tauri-apps/api/event";
import { Download } from "lucide-react";
import { useAppState } from "@/app/state";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import { errorText } from "@/lib/utils";
import { platformApi, UPDATE_INSTALL_EVENT, type InstallStatus } from "@/views/platform-api";
import { FieldError, OutcomeLine, Row } from "./fields";
import { useSave } from "./save";

const SPINNER = { "aria-hidden": true, role: undefined, "aria-label": undefined } as const;

/**
 * Settings > Updates and about, inside the Updates group: the "Install updates automatically"
 * switch and, while an update is downloading, waiting or failed, its state and "Install and
 * restart now". Backend: platform_extras/autoupdate.rs.
 */
export function AutoInstallRows() {
  const app = useAppState();
  const on = app.settings.settings.autoInstallUpdates ?? true;
  const save = useSave();
  const id = React.useId();
  const [install, setInstall] = React.useState<InstallStatus | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [starting, setStarting] = React.useState(false);

  React.useEffect(() => {
    platformApi
      .info()
      .then((i) => setInstall(i.install ?? null))
      .catch(() => {});
    const un = listen<InstallStatus>(UPDATE_INSTALL_EVENT, (e) => setInstall(e.payload));
    return () => void un.then((f) => f());
  }, []);

  async function installNow() {
    setError(null);
    setStarting(true);
    try {
      // On success Daytrace quits for the installer and this never returns.
      await platformApi.installUpdate();
    } catch (e) {
      setError(`Couldn’t install the update: ${errorText(e)}`);
    } finally {
      setStarting(false);
    }
  }

  const failed = install?.stage === "failed" ? install.error : null;

  return (
    <>
      <Row
        label="Install updates automatically"
        htmlFor={`${id}-auto`}
        description="Daytrace downloads a new version, checks its signature and installs it while the PC has been idle for 10 minutes or is locked, then starts again in the tray. When this is off, you get a notification and the release page instead."
      >
        <Switch
          id={`${id}-auto`}
          checked={on}
          onCheckedChange={(v) => {
            setError(null);
            save({ autoInstallUpdates: v }).catch((e) => setError(`Couldn’t save: ${errorText(e)}`));
          }}
        />
      </Row>
      {install && install.stage !== "idle" && (
        <Row label="New version" description={<span aria-live="polite">{installText(install, on)}</span>}>
          {install.stage === "ready" && (
            <Button variant="outline" onClick={() => void installNow()} disabled={starting}>
              {starting ? <Spinner {...SPINNER} /> : <Download aria-hidden />}
              {starting ? "Starting the installer…" : "Install and restart now"}
            </Button>
          )}
        </Row>
      )}
      {/* Always present (screen readers announce what is added to it); out of view when empty. */}
      <div aria-live="polite" className={failed || error ? "flex flex-col gap-2 px-4 py-3" : "sr-only"}>
        {failed && <OutcomeLine outcome={{ ok: false, text: failed }} />}
        {error && <FieldError>{error}</FieldError>}
      </div>
    </>
  );
}

function installText(s: InstallStatus, auto: boolean): React.ReactNode {
  const v = s.version ? `version ${s.version}` : "the new version";
  switch (s.stage) {
    case "downloading": {
      const pct = s.total ? Math.min(100, Math.round((s.downloaded / s.total) * 100)) : null;
      return (
        <span className="inline-flex items-center gap-2">
          <Spinner className="size-3.5" {...SPINNER} />
          {`Downloading ${v}…${pct !== null ? ` ${pct}%` : ""}`}
        </span>
      );
    }
    case "ready":
      return auto
        ? `Daytrace downloaded ${v}. It installs when the PC has been idle for 10 minutes or is locked.`
        : `Daytrace downloaded ${v}. It installs when you select Install and restart now.`;
    case "installing":
      return `Installing ${v}. Daytrace closes and starts again.`;
    case "failed":
      return `Couldn’t download ${v}. The release page has the installer.`;
    default:
      return null;
  }
}
