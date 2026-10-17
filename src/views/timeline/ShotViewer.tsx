import * as React from "react";
import { ChevronLeft, ChevronRight, EyeOff, FolderOpen, X } from "lucide-react";
import { Dialog as DialogPrimitive } from "radix-ui";
import { Button } from "@/components/ui/button";
import { DialogOverlay, DialogPortal } from "@/components/ui/dialog";
import { Kbd } from "@/components/ui/kbd";
import { Spinner } from "@/components/ui/spinner";
import { clock } from "@/lib/time";
import { errorText } from "@/lib/utils";
import { timelineApi, type Card, type Shot } from "./api";
import { fullImage } from "./shots";

/*
 * Full-size screenshot viewer (designer-bespoke): an opaque layer over the whole window, so the
 * screenshot is not read against the app behind it. Time and window title, "7 of 21", the card
 * title, "Show in File Explorer" (the backend resolves the file; no path reaches the UI),
 * previous/next buttons and ←/→, Home/End, Esc closes and focus returns to where it came from.
 */

export function ShotViewer({
  card,
  frames,
  index,
  onIndex,
}: {
  card: Card;
  frames: Shot[];
  index: number | null;
  onIndex: (i: number | null) => void;
}) {
  const shot = index !== null ? (frames[index] ?? null) : null;
  const [img, setImg] = React.useState<{ id: number; src: string | null } | null>(null);
  const [reveal, setReveal] = React.useState<{ busy: boolean; error: string | null }>({ busy: false, error: null });

  React.useEffect(() => {
    setReveal({ busy: false, error: null });
    if (!shot || shot.hiddenApp) return;
    let live = true;
    void fullImage(shot.id).then((src) => {
      if (!live) return;
      setImg({ id: shot.id, src });
      // Have the next one ready for →.
      const next = index !== null ? frames[index + 1] : undefined;
      if (next && !next.hiddenApp) void fullImage(next.id);
    });
    return () => {
      live = false;
    };
  }, [shot, index, frames]);

  const go = (i: number) => {
    if (i >= 0 && i < frames.length) onIndex(i);
  };

  const showInExplorer = async () => {
    if (!shot) return;
    setReveal({ busy: true, error: null });
    try {
      await timelineApi.revealScreenshot(shot.id);
      setReveal({ busy: false, error: null });
    } catch (e) {
      setReveal({ busy: false, error: errorText(e) });
    }
  };

  const src = shot && img?.id === shot.id ? img.src : undefined;

  return (
    <DialogPrimitive.Root open={index !== null} onOpenChange={(o) => !o && onIndex(null)}>
      <DialogPortal>
        <DialogOverlay className="bg-background" />
        <DialogPrimitive.Content
          className="fixed inset-0 z-50 flex flex-col bg-background text-foreground outline-none data-[state=open]:animate-in data-[state=open]:fade-in-0"
          onKeyDown={(e) => {
            if (index === null) return;
            const t = e.target as HTMLElement;
            if (t.tagName === "BUTTON" && (e.key === "Enter" || e.key === " ")) return;
            if (e.key === "ArrowRight") go(index + 1);
            else if (e.key === "ArrowLeft") go(index - 1);
            else if (e.key === "Home") go(0);
            else if (e.key === "End") go(frames.length - 1);
            else return;
            e.preventDefault();
          }}
        >
          <div className="flex flex-wrap items-center gap-x-4 gap-y-2 border-b px-4 py-2.5">
            <div className="min-w-0 flex-1">
              <DialogPrimitive.Title className="truncate text-body font-semibold">
                <span className="font-mono tnum">{shot ? clock(shot.capturedAt) : ""}</span>
                {shot?.windowTitle && (
                  <span className="font-normal text-muted-foreground" translate="no">
                    {" "}
                    · {shot.windowTitle}
                  </span>
                )}
              </DialogPrimitive.Title>
              <p className="truncate text-caption text-muted-foreground">
                From the card “{card.title}”, {card.start} – {card.end}
              </p>
              <DialogPrimitive.Description className="sr-only">
                Screenshot {(index ?? 0) + 1} of {frames.length}. Left and right arrow keys move, Escape closes.
              </DialogPrimitive.Description>
            </div>
            <span className="font-mono text-meta text-muted-foreground tnum" aria-hidden="true">
              {(index ?? 0) + 1} of {frames.length}
            </span>
            <Button variant="outline" size="sm" onClick={showInExplorer} disabled={!shot || !!shot.hiddenApp || src === null || reveal.busy}>
              {reveal.busy ? <Spinner className="size-3.5" aria-hidden="true" role="presentation" /> : <FolderOpen aria-hidden="true" />}
              Show in File Explorer
            </Button>
            <DialogPrimitive.Close asChild>
              <Button variant="ghost" size="icon-sm" aria-label="Close viewer">
                <X aria-hidden="true" />
              </Button>
            </DialogPrimitive.Close>
            {reveal.error && (
              <p role="alert" className="basis-full text-meta text-destructive">
                Couldn’t show the file in File Explorer: {reveal.error}
              </p>
            )}
          </div>

          <div className="relative grid min-h-0 flex-1 place-items-center px-16 py-4">
            {!shot ? null : shot.hiddenApp ? (
              <div className="hatch-gap grid aspect-video w-full max-w-4xl place-items-center rounded-md border border-dashed text-body text-muted-foreground">
                <span className="inline-flex items-center gap-2 rounded-sm bg-surface-1 px-2 py-1">
                  <EyeOff className="size-4" aria-hidden="true" />
                  <span>
                    <span translate="no">{shot.hiddenApp}</span> was in front; not captured
                  </span>
                </span>
              </div>
            ) : src ? (
              <img src={src} alt={`Screen at ${clock(shot.capturedAt)}`} className="max-h-full max-w-full rounded-md border object-contain shadow-md" draggable={false} />
            ) : src === null ? (
              <p className="text-body text-muted-foreground">This screenshot was deleted (retention period or storage limit).</p>
            ) : (
              <Spinner className="size-6 text-muted-foreground" aria-label="Loading the screenshot" />
            )}
            <Button
              variant="outline"
              size="icon-lg"
              className="absolute top-1/2 left-3 -translate-y-1/2"
              aria-label="Previous screenshot"
              aria-keyshortcuts="ArrowLeft"
              onClick={() => index !== null && go(index - 1)}
              disabled={index === null || index <= 0}
            >
              <ChevronLeft aria-hidden="true" />
            </Button>
            <Button
              variant="outline"
              size="icon-lg"
              className="absolute top-1/2 right-3 -translate-y-1/2"
              aria-label="Next screenshot"
              aria-keyshortcuts="ArrowRight"
              onClick={() => index !== null && go(index + 1)}
              disabled={index === null || index >= frames.length - 1}
            >
              <ChevronRight aria-hidden="true" />
            </Button>
          </div>
          <p className="flex items-center justify-center gap-1.5 border-t px-4 py-2 text-caption text-muted-foreground">
            <Kbd>←</Kbd> <Kbd>→</Kbd> move, <Kbd>Esc</Kbd> closes
          </p>
        </DialogPrimitive.Content>
      </DialogPortal>
    </DialogPrimitive.Root>
  );
}
