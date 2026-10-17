import * as React from "react";
import { listen } from "@tauri-apps/api/event";
import { Kbd } from "@/components/ui/kbd";
import { Spinner } from "@/components/ui/spinner";
import { useKeyLabel } from "@/lib/keys";
import { useDelayedLoading } from "@/lib/use-delayed";
import { frameUrl } from "@/views/agents/api";

/*
 * The AgentPlayback dashboard frame outlives the Agents section. The shell mounts
 * <AgentsFrameHost/> once inside <main>; the section only marks the spot (<AgentsFrame/>) and the
 * host lays the frame over it. Leaving the section hides the frame (display: none keeps its
 * document, scroll position and state), coming back shows it again without a reload. Moving an
 * iframe in the DOM would reload it, so it never moves; only its box follows the spot.
 *
 * Nothing leaks: the frame goes away when the dashboard stops (the section sees it and calls
 * agentsFrame.drop()), and with the window, which Daytrace destroys when it goes to the tray.
 * While it is hidden the dashboard page keeps running as it would in a browser tab. If the
 * dashboard process ends while another section is open, the frame shows its last page until the
 * Agents section is opened again and drops it.
 *
 * Keys typed inside the frame stay in the frame's document (another origin). The backend watches
 * the WebView's keys (features/agents.rs watch_frame_keys, Windows) and sends Esc, Ctrl+K,
 * Ctrl+1…9 and Ctrl+, here; while the focus is inside the frame, Esc moves it back to the page and
 * the shortcuts run as usual. Nothing is injected into the dashboard.
 */

/** Backend event with a shell key's KeyboardEvent.code (features/agents.rs FRAME_KEY_EVENT). */
export const FRAME_KEY_EVENT = "agents-frame-key";
/** The group around the frame; Esc inside the frame moves the focus here. */
const REGION_ID = "agents-frame";

interface FrameState {
  /** The dashboard's address while it runs. */
  url: string | null;
  /** The Agents section's spot for the frame while the section is open. */
  slot: HTMLElement | null;
  loaded: boolean;
  /** The keyboard focus is inside the frame. */
  focused: boolean;
}

let current: FrameState = { url: null, slot: null, loaded: false, focused: false };
const listeners = new Set<() => void>();

function update(patch: Partial<FrameState>) {
  const next = { ...current, ...patch };
  if ((Object.keys(next) as (keyof FrameState)[]).every((k) => next[k] === current[k])) return;
  current = next;
  listeners.forEach((l) => l());
}

function useFrameState(): FrameState {
  return React.useSyncExternalStore(
    (cb) => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    () => current,
  );
}

export const agentsFrame = {
  /** Shows the dashboard at `url` over `slot` (a new address loads it again). */
  show(url: string, slot: HTMLElement) {
    update(url === current.url ? { slot } : { url, slot, loaded: false, focused: false });
  },
  /** The section closed: the frame stays loaded, hidden. */
  hide(slot: HTMLElement) {
    if (current.slot === slot) update({ slot: null, focused: false });
  },
  /** The dashboard stopped: the frame goes away. */
  drop() {
    update({ url: null, slot: null, loaded: false, focused: false });
  },
};

const KEY_OF: Record<string, string> = { KeyK: "k", Comma: "," };

/** Mounted once by the shell, inside <main> (positioned), after the section. */
export function AgentsFrameHost() {
  const { url, slot, loaded } = useFrameState();
  const src = url ? frameUrl(url) : null;
  const boxRef = React.useRef<HTMLDivElement>(null);
  const frameRef = React.useRef<HTMLIFrameElement>(null);
  const [box, setBox] = React.useState<{ top: number; left: number; width: number; height: number } | null>(null);
  const showLoading = useDelayedLoading(!!slot && !loaded);

  // Follow the spot: its size changes with the window and with banners above it.
  React.useLayoutEffect(() => {
    const el = boxRef.current;
    const parent = el?.parentElement;
    if (!slot || !el || !parent) {
      setBox(null);
      return;
    }
    const place = () => {
      const p = parent.getBoundingClientRect();
      const r = slot.getBoundingClientRect();
      const next = { top: r.top - p.top - parent.clientTop, left: r.left - p.left - parent.clientLeft, width: r.width, height: r.height };
      setBox((b) => (b && b.top === next.top && b.left === next.left && b.width === next.width && b.height === next.height ? b : next));
    };
    place();
    const ro = new ResizeObserver(place);
    ro.observe(slot);
    ro.observe(parent);
    addEventListener("resize", place);
    return () => {
      ro.disconnect();
      removeEventListener("resize", place);
    };
  }, [slot, src]);

  // Whether the focus is inside the frame (the parent document's activeElement is the iframe).
  React.useEffect(() => {
    if (!src) return;
    const check = () => update({ focused: !!frameRef.current && document.activeElement === frameRef.current });
    const later = () => setTimeout(check, 0);
    addEventListener("blur", later);
    addEventListener("focus", check);
    document.addEventListener("focusin", check);
    return () => {
      removeEventListener("blur", later);
      removeEventListener("focus", check);
      document.removeEventListener("focusin", check);
    };
  }, [src]);

  // Shell keys pressed while the focus is inside the frame.
  React.useEffect(() => {
    if (!src) return;
    let alive = true;
    let unlisten: (() => void) | undefined;
    void listen<string>(FRAME_KEY_EVENT, (e) => {
      const frame = frameRef.current;
      if (!frame || document.activeElement !== frame) return;
      const code = e.payload;
      // Out of the frame first: Esc stops there; a shortcut then runs from the page.
      (document.getElementById(REGION_ID) ?? document.getElementById("content"))?.focus();
      if (code === "Escape") return;
      dispatchEvent(new KeyboardEvent("keydown", { code, key: KEY_OF[code] ?? code.replace(/^Digit/, ""), ctrlKey: true, bubbles: true, cancelable: true }));
    })
      .then((u) => {
        if (alive) unlisten = u;
        else u();
      })
      .catch(() => undefined);
    return () => {
      alive = false;
      unlisten?.();
    };
  }, [src]);

  if (!src) return null;
  return (
    <div ref={boxRef} hidden={!slot || !box} className="absolute" style={box ?? undefined}>
      {showLoading && (
        <p className="pointer-events-none absolute inset-x-0 top-6 flex items-center justify-center gap-2 text-meta text-muted-foreground">
          <Spinner aria-hidden="true" role={undefined} aria-label={undefined} /> Loading the dashboard…
        </p>
      )}
      <iframe
        key={src}
        ref={frameRef}
        title="AgentPlayback dashboard"
        src={src}
        // Scripts and its own storage on its own origin; popups only reach the backend's
        // new-window handler, which opens web links in the default browser. No navigating Daytrace.
        sandbox="allow-scripts allow-same-origin allow-popups"
        referrerPolicy="no-referrer"
        onLoad={() => update({ loaded: true })}
        className="relative size-full border-0"
      />
    </div>
  );
}

/** The Agents section's spot for the dashboard, with the line that says where the keys go. */
export function AgentsFrame({ url }: { url: string }) {
  const [slot, setSlot] = React.useState<HTMLDivElement | null>(null);
  const { focused } = useFrameState();
  const k = useKeyLabel("KeyK");

  React.useLayoutEffect(() => {
    if (!slot) return;
    agentsFrame.show(url, slot);
    return () => agentsFrame.hide(slot);
  }, [url, slot]);

  return (
    <div
      id={REGION_ID}
      role="group"
      aria-label="AgentPlayback dashboard"
      tabIndex={-1}
      className="flex min-h-0 flex-1 flex-col focus:outline-2 focus:-outline-offset-2 focus:outline-ring"
    >
      <p className="min-h-8 shrink-0 border-b px-(--page-x) py-1.5 text-caption text-muted-foreground" aria-live="polite">
        {focused ? (
          <>
            Keys go to the dashboard. <Kbd>Esc</Kbd> returns to Daytrace; <Kbd>Ctrl+{k}</Kbd> and <Kbd>Ctrl+1</Kbd>…<Kbd>Ctrl+6</Kbd> still work.
          </>
        ) : (
          <>Served by AgentPlayback on this PC. Its links open in your browser.</>
        )}
      </p>
      <div ref={setSlot} className="min-h-0 flex-1" />
    </div>
  );
}
