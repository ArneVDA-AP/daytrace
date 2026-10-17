import * as React from "react";
import { ExternalLink } from "lucide-react";
import { shellApi } from "@/app/shell-api";
import { Button } from "@/components/ui/button";
import { Dialog, DialogClose, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { toast } from "@/components/ui/toast";
import { errorText } from "@/lib/utils";
import { platformApi, type WhatsNew as Notes } from "@/views/platform-api";

/** Links in the notes go through the one link opener, which allows Daytrace's own pages and a few help sites. */
function openLink(url: string) {
  shellApi.openUrl(url).catch((e) => toast("Couldn’t open the link", { description: errorText(e), tone: "error" }));
}

let settle: () => void = () => {};
/** Resolves once "What's new" has nothing to show or was closed: the goal prompt waits for it. */
export const whatsNewSettled = new Promise<void>((resolve) => (settle = resolve));

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
        if (!info) settle();
        if (!info || !alive) return;
        // The chat's renderer: React elements only, never innerHTML. Loaded only when needed.
        const { renderMarkdown } = await import("@/views/chat/markdown");
        if (!alive) return;
        const { nodes } = renderMarkdown(info.notes, {
          cardChip: () => null,
          onExternalLink: openLink,
          // "### New in the window" under the dialog's h2 title is an h3.
          headingOffset: 0,
        });
        setNotes({ info, nodes });
        setOpen(true);
        void platformApi.whatsNewSeen(info.version).catch(() => {});
      } catch {
        // Nothing to show; the next start asks again.
        settle();
      }
    })();
    return () => {
      alive = false;
    };
  }, []);

  if (!notes) return null;
  const { info, nodes } = notes;
  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (!o) settle();
      }}
    >
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
            <Button variant="outline" onClick={() => openLink(info.releaseUrl!)}>
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
