import * as React from "react";
import { ClipboardCopy } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Spinner } from "@/components/ui/spinner";
import { errorText } from "@/lib/utils";
import { platformApi } from "@/views/platform-api";
import { Group, OutcomeLine, Row, type Outcome } from "./fields";

const SPINNER = { "aria-hidden": true, role: undefined, "aria-label": undefined } as const;

/**
 * Settings > Updates and about > Troubleshooting: "Copy debug info" (Dayflow's "Copy debug logs").
 * The text comes from platform_extras/debug_info.rs with keys, the user folder and titles taken out.
 */
export function DebugInfoGroup() {
  const [busy, setBusy] = React.useState(false);
  const [outcome, setOutcome] = React.useState<Outcome | null>(null);

  async function copy() {
    setBusy(true);
    setOutcome(null);
    try {
      const text = await platformApi.debugInfo();
      await navigator.clipboard.writeText(text);
      const kb = Math.max(1, Math.round(new Blob([text]).size / 1024));
      setOutcome({ ok: true, text: `Copied ${kb} KB of debug info. Paste it into an issue or a Claude session.` });
    } catch (e) {
      setOutcome({ ok: false, text: `Couldn’t copy the debug info: ${errorText(e)}` });
    } finally {
      setBusy(false);
    }
  }

  return (
    <Group title="Troubleshooting">
      <Row
        label="Copy debug info"
        description="The recent batches with their status, the recent model calls (model, time taken, tokens, errors) and the end of the log, as text. API keys, your user folder and window and card titles are left out; screenshots and what the model wrote are never included."
      >
        <Button variant="outline" onClick={() => void copy()} disabled={busy}>
          {busy ? <Spinner {...SPINNER} /> : <ClipboardCopy aria-hidden />}
          {busy ? "Copying…" : "Copy debug info"}
        </Button>
      </Row>
      {/* Always present (screen readers announce what is added to it); out of view when empty. */}
      <div aria-live="polite" className={outcome ? "px-4 py-3" : "sr-only"}>
        {outcome && <OutcomeLine outcome={outcome} />}
      </div>
    </Group>
  );
}
