import * as React from "react";
import { ExternalLink } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { platformApi, type WhatsNew as Notes } from "@/views/platform-api";

/**
 * "What's new" once after an update (Dayflow's WhatsNewView): the notes of the running version,
 * from its GitHub release or, offline, from the CHANGELOG that came with it. Mounted after the
 * first-run setup; marked as seen as soon as it opens, so it shows once. Backend:
 * src-tauri/src/features/platform_extras/whats_new.rs.
 */
export function WhatsNew() {
  const [notes, setNotes] = React.useState<{ info: Notes; nodes: React.ReactNode[] } | null>(null);
  const [open, setOpen] = React.useState(false);
  const closeRef = React.useRef<HTMLButtonElement>(null);

  React.useEffect(() => {
    let alive = true;
    void (async () => {
      try {
        const info = await platformApi.whatsNew();
        if (!info || !alive) return;
        // The chat's renderer: React elements only, never innerHTML. Loaded only when needed.
        const { renderMarkdown } = await import("@/views/chat/markdown");
        if (!alive) return;
        const { nodes } = renderMarkdown(info.notes, {
          cardChip: () => null,
          onExternalLink: (url) => void platformApi.openLink(url).catch(() => {}),
        });
        setNotes({ info, nodes });
        setOpen(true);
        void platformApi.whatsNewSeen(info.version).catch(() => {});
      } catch {
        // Nothing to show; the next start asks again.
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  if (!notes) return null;
  const { info, nodes } = notes;
  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogContent
        className="gap-5 sm:max-w-[34rem]"
        onOpenAutoFocus={(e) => {
          // Start on Close rather than on the notes (the first focusable element).
          e.preventDefault();
          closeRef.current?.focus();
        }}
      >
        <DialogHeader>
          <DialogTitle className="font-display text-title font-semibold">What’s new in Daytrace {info.version}</DialogTitle>
          <DialogDescription className="text-meta">
            {info.source === "release" ? "From the release notes on GitHub." : "From the notes that came with this version."}
          </DialogDescription>
        </DialogHeader>
        <section
          aria-label="Release notes"
          tabIndex={0}
          className="scroll-thin -mx-1 flex max-h-[min(60vh,26rem)] flex-col gap-3 overflow-y-auto px-1 text-body leading-6 [overflow-wrap:anywhere] focus-visible:outline-offset-2"
        >
          {nodes}
        </section>
        <DialogFooter>
          {info.releaseUrl && (
            <Button variant="outline" onClick={() => void platformApi.openLink(info.releaseUrl!).catch(() => {})}>
              <ExternalLink aria-hidden /> Open the release page
            </Button>
          )}
          <DialogClose asChild>
            <Button ref={closeRef}>Close</Button>
          </DialogClose>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
