import * as React from "react";
import { api, type ProviderKind } from "@/api";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import { errorText } from "@/lib/utils";
import { OutcomeLine, type Outcome } from "./fields";
import { cliCommand } from "./providers";

type InputBinding = Pick<React.ComponentProps<"input">, "onBlur" | "onKeyDown" | "aria-invalid"> & {
  value: string;
  onChange: (e: React.ChangeEvent<HTMLInputElement>) => void;
};

/**
 * Path of the claude / codex executable (empty = find it automatically), with a check that runs
 * `--version`. The caller labels it (`id`) and decides when the path is saved (`input`).
 */
export function CliPathField({ id, provider, input, describedBy }: { id: string; provider: ProviderKind; input: InputBinding; describedBy?: string }) {
  const [outcome, setOutcome] = React.useState<Outcome | null>(null);
  const [busy, setBusy] = React.useState(false);
  const exe = cliCommand(provider);
  const path = input.value;

  // A new path or provider makes the last check meaningless.
  React.useEffect(() => setOutcome(null), [path, provider]);

  async function check() {
    setBusy(true);
    setOutcome(null);
    try {
      const found = await api.providerCheckCli(provider, path);
      setOutcome({ ok: true, text: `${`Found ${exe} ${found.version}`.trimEnd()} at ${found.path}` });
    } catch (e) {
      setOutcome({ ok: false, text: errorText(e) });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="flex w-full flex-col gap-2">
      <div className="flex w-full flex-wrap gap-2">
        <Input
          id={id}
          {...input}
          placeholder={`Find ${exe} automatically`}
          spellCheck={false}
          autoComplete="off"
          aria-describedby={describedBy}
          className="min-w-0 flex-1 basis-64 font-mono text-meta placeholder:font-sans"
        />
        <Button type="button" variant="outline" onClick={check} disabled={busy}>
          {busy && <Spinner aria-hidden role={undefined} aria-label={undefined} />}
          {busy ? "Checking…" : `Find ${exe}`}
        </Button>
      </div>
      <div aria-live="polite">
        <OutcomeLine outcome={outcome} />
      </div>
    </div>
  );
}
