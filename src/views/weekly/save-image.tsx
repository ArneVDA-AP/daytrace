import * as React from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { ImageDown } from "lucide-react";
import { Banner } from "@/components/banner";
import { Button } from "@/components/ui/button";
import { toast } from "@/components/ui/toast";
import { errorText } from "@/lib/utils";
import { insightsApi } from "@/views/insights/api";
import { RetryButton } from "@/views/insights/parts";
import { chartPng } from "./export-image";

/**
 * "Save as PNG…" for one Weekly chart: `ref` goes on the element that holds the chart and its
 * legend, `button` in the card's actions, `error` under the chart (an inline banner with Try again).
 */
export function useChartImage({ title, subtitle, file }: { title: string; subtitle: string; file: string }) {
  const ref = React.useRef<HTMLDivElement>(null);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const run = async () => {
    const el = ref.current;
    if (!el || busy) return;
    setBusy(true);
    setError(null);
    try {
      const png = await chartPng(el, title, subtitle);
      const path = await save({ defaultPath: file, filters: [{ name: "PNG image", extensions: ["png"] }] });
      if (!path) return;
      await insightsApi.saveImage(path, png);
      toast("Image saved", { description: path, tone: "success" });
    } catch (e) {
      setError(errorText(e));
    } finally {
      setBusy(false);
    }
  };

  const button = (
    <Button variant="ghost" size="sm" onClick={() => void run()} disabled={busy}>
      <ImageDown aria-hidden="true" /> {busy ? "Saving…" : "Save as PNG…"}
    </Button>
  );
  const errorBanner = error ? (
    <Banner tone="error" title="Couldn’t save the image" details={error} actions={<RetryButton onClick={() => void run()} busy={busy} />} className="mt-3">
      Check that the folder can be written to, or pick another one.
    </Banner>
  ) : null;
  return { ref, button, error: errorBanner };
}
