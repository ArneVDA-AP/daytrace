import * as React from "react";
import { EyeOff, Maximize2, Pause, Play, SkipBack } from "lucide-react";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { ToggleGroup, ToggleGroupItem } from "@/components/ui/toggle-group";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { clock, formatDuration } from "@/lib/time";
import { cn } from "@/lib/utils";
import { timelineApi, type Card, type Shot } from "./api";
import { readPref, writePref } from "./data";
import { cachedThumb, clearWantedThumbs, hasThumb, useThumbTick, wantThumbs } from "./shots";
import { ShotViewer } from "./ShotViewer";

/*
 * A card's screenshots as one control (designer-bespoke): the timelapse stage, its controls, and a
 * contact strip of the card's screenshots that jumps the playhead. Playback follows Dayflow's
 * screenshot slideshow (Views/UI/MainView/ScreenshotSlideshow*.swift): frames at their real spacing
 * sped up 20×, 40× or 60× (remembered), held on a frame until its image has loaded. It plays by
 * itself only with `autoPlay` (review mode, as Dayflow's review card does) and not under reduced
 * motion. Keys inside: Space plays and pauses, ← and → step one frame (unless `arrowKeys` is off:
 * review mode uses them to move between cards); Full size opens the viewer. The contact strip
 * (`strip`, on by default) is one tab stop: ← → Home End move along it and jump the playhead
 * (UX10). Thumbnails load a few at a time, ahead of the playhead; closing the player drops the
 * rest of the queue (FE11). A change of `toggleToken` plays or pauses (review mode's Space).
 */

const SPEEDS = [20, 40, 60];
const SPEED_KEY = "daytrace.timelapseSpeed";
/** Frames played: an even sample over the whole card, picked by the backend. */
const MAX_FRAMES = 600;
const PREFETCH = 8;
const STRIP = 12;

const isFrame = (s: Shot) => !s.hiddenApp;

/** `n` items spread evenly over the list. */
function sample<T>(items: T[], n: number): T[] {
  if (items.length <= n) return items;
  const out: T[] = [];
  for (let i = 0; i < n; i++) out.push(items[Math.round((i * (items.length - 1)) / (n - 1))]);
  return out;
}

function loadSpeed(): number {
  const v = Number(readPref(SPEED_KEY));
  return SPEEDS.includes(v) ? v : SPEEDS[0];
}

export function Timelapse({
  card,
  autoPlay = false,
  arrowKeys = true,
  strip: showStrip = true,
  toggleToken,
  className,
}: {
  card: Card;
  /** Start playing once the frames are known (not under prefers-reduced-motion). */
  autoPlay?: boolean;
  /** ← and → step one frame. */
  arrowKeys?: boolean;
  /** The contact strip of screenshots under the controls. */
  strip?: boolean;
  /** Each new value plays or pauses. */
  toggleToken?: number;
  className?: string;
}) {
  const [frames, setFrames] = React.useState<Shot[] | null>(null);
  const [error, setError] = React.useState(false);
  const [index, setIndex] = React.useState(0);
  const [playing, setPlaying] = React.useState(false);
  const [speed, setSpeed] = React.useState(loadSpeed);
  const [viewer, setViewer] = React.useState<number | null>(null);
  useThumbTick();

  React.useEffect(() => {
    let live = true;
    setFrames(null);
    setError(false);
    setIndex(0);
    setPlaying(false);
    setViewer(null);
    timelineApi
      .cardScreenshots(card.id, MAX_FRAMES)
      .then((s) => live && setFrames(s))
      .catch(() => {
        if (!live) return;
        setFrames([]);
        setError(true);
      });
    return () => {
      live = false;
    };
  }, [card.id]);

  const strip = React.useMemo(() => (frames && showStrip ? sample(frames, STRIP) : []), [frames, showStrip]);

  // Review mode: play as soon as the frames are known.
  React.useEffect(() => {
    if (!autoPlay || !frames || frames.length < 2) return;
    if (matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    setPlaying(true);
  }, [autoPlay, frames]);

  // Prefetch: the next few frames from the playhead, then the contact strip.
  React.useEffect(() => {
    if (!frames || frames.length === 0) return;
    const ahead = frames.slice(index, index + PREFETCH + 1).filter(isFrame).map((s) => s.id);
    wantThumbs([...ahead, ...strip.filter(isFrame).map((s) => s.id)]);
  }, [frames, index, strip]);
  React.useEffect(() => clearWantedThumbs, []);
  const stripRef = React.useRef<HTMLUListElement>(null);

  // Playback: wait the real gap to the next frame divided by the speed; hold on a frame whose
  // image has not loaded yet (this runs again when it arrives, through useThumbTick).
  const next = frames && index < frames.length - 1 ? frames[index + 1] : null;
  const nextReady = !!next && (!isFrame(next) || hasThumb(next.id));
  React.useEffect(() => {
    if (!playing || !frames) return;
    if (index >= frames.length - 1) {
      setPlaying(false);
      return;
    }
    if (!nextReady) return;
    const gap = Math.min(120, Math.max(1, frames[index + 1].capturedAt - frames[index].capturedAt));
    const t = setTimeout(() => setIndex((i) => Math.min(i + 1, frames.length - 1)), (gap * 1000) / speed);
    return () => clearTimeout(t);
  }, [playing, frames, index, speed, nextReady]);

  const togglePlay = () => {
    if (!frames || frames.length < 2) return;
    if (!playing && index >= frames.length - 1) setIndex(0);
    setPlaying((p) => !p);
  };
  const toggleRef = React.useRef(togglePlay);
  toggleRef.current = togglePlay;
  const lastToken = React.useRef(toggleToken);
  React.useEffect(() => {
    if (toggleToken === lastToken.current) return;
    lastToken.current = toggleToken;
    toggleRef.current();
  }, [toggleToken]);

  if (frames === null) {
    return (
      <div aria-busy="true" aria-label="Loading screenshots" className={cn("flex max-w-[44rem] flex-col gap-2", className)}>
        <Skeleton className="aspect-video w-full" />
        <Skeleton className="h-8 w-full" />
      </div>
    );
  }
  if (frames.length === 0) {
    return (
      <p className="rounded-md border border-dashed px-3 py-4 text-meta text-muted-foreground">
        {error ? "Couldn’t load the screenshots of this card." : "No screenshots were kept for this card. They are deleted after the retention period or when the storage limit is reached."}
      </p>
    );
  }

  const cur = frames[Math.min(index, frames.length - 1)];
  const src = isFrame(cur) ? cachedThumb(cur.id) : undefined;

  const step = (d: number) => {
    setPlaying(false);
    setIndex((i) => Math.max(0, Math.min(frames.length - 1, i + d)));
  };
  const changeSpeed = (v: string) => {
    const n = Number(v);
    if (!SPEEDS.includes(n)) return;
    setSpeed(n);
    writePref(SPEED_KEY, String(n));
  };
  const openFull = (i: number) => {
    setPlaying(false);
    setViewer(i);
  };

  // Space plays and pauses (a focused button keeps its own Space); ← → step one frame (the
  // scrubber and the strip keep their own arrows). Keys from the full-size viewer reach this
  // handler through the React tree (it is a portal): they are the viewer's, not the player's (FE6).
  const onKey = (e: React.KeyboardEvent) => {
    if (!e.currentTarget.contains(e.target as Node)) return;
    const t = e.target as HTMLElement;
    const isRange = t instanceof HTMLInputElement && t.type === "range";
    const inGroup = !!t.closest('[role="group"],[role="radiogroup"],[data-strip]');
    if (e.key === " " && !(t instanceof HTMLButtonElement)) {
      e.preventDefault();
      togglePlay();
    } else if (arrowKeys && (e.key === "ArrowRight" || e.key === "ArrowLeft") && !isRange && !inGroup) {
      e.preventDefault();
      step(e.key === "ArrowRight" ? 1 : -1);
    }
  };

  const seconds = (card.endTs - card.startTs) / speed;
  // The strip's one tab stop: the last strip frame at or before the playhead.
  let stripStop = 0;
  strip.forEach((f, n) => {
    if (frames.indexOf(f) <= index) stripStop = n;
  });

  return (
    <section aria-label="Screenshots and timelapse" className={cn("flex max-w-[44rem] flex-col gap-2", className)} onKeyDown={onKey}>
      <div className="relative aspect-video w-full overflow-hidden rounded-md border bg-surface-2">
        <div
          tabIndex={0}
          role="img"
          aria-roledescription="timelapse frame"
          aria-label={
            isFrame(cur)
              ? `Screen at ${clock(cur.capturedAt)}${cur.windowTitle ? `, ${cur.windowTitle}` : ""}. Space plays or pauses${arrowKeys ? ", arrow keys step" : ""}.`
              : `${cur.hiddenApp} was in front at ${clock(cur.capturedAt)}; not captured.`
          }
          className="absolute inset-0 rounded-md focus-visible:-outline-offset-2"
          onDoubleClick={() => isFrame(cur) && openFull(index)}
        >
          {!isFrame(cur) ? (
            <div className="hatch-gap grid h-full place-items-center text-meta text-muted-foreground">
              <span className="inline-flex items-center gap-1.5 rounded-sm bg-surface-1 px-2 py-1">
                <EyeOff className="size-4" aria-hidden="true" />
                <span>
                  <span translate="no">{cur.hiddenApp}</span> was in front; not captured
                </span>
              </span>
            </div>
          ) : src ? (
            <img src={src} alt="" className="block h-full w-full object-contain" draggable={false} />
          ) : (
            <div className="grid h-full place-items-center text-meta text-muted-foreground">{src === null ? "Screenshot deleted" : "Loading…"}</div>
          )}
          <span className="absolute top-2 left-2 rounded-sm bg-popover/90 px-1.5 py-0.5 font-mono text-caption text-popover-foreground shadow-xs tnum">
            {clock(cur.capturedAt)}
          </span>
        </div>
        <Tooltip>
          <TooltipTrigger asChild>
            <Button
              variant="outline"
              size="icon-sm"
              className="absolute top-2 right-2 bg-popover/90"
              aria-label="Full size"
              disabled={!isFrame(cur)}
              onClick={() => openFull(index)}
            >
              <Maximize2 aria-hidden="true" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Full size</TooltipContent>
        </Tooltip>
      </div>

      <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
        <div className="flex items-center gap-1">
          <Button size="icon" onClick={togglePlay} disabled={frames.length < 2} aria-label={playing ? "Pause timelapse" : "Play timelapse"} aria-keyshortcuts="Space">
            {playing ? <Pause aria-hidden="true" /> : <Play aria-hidden="true" />}
          </Button>
          <Button size="icon-sm" variant="ghost" aria-label="Back to the first frame" onClick={() => step(-frames.length)} disabled={index === 0}>
            <SkipBack aria-hidden="true" />
          </Button>
        </div>
        <div className="flex min-w-[10rem] flex-1 flex-col gap-0.5">
          <input
            type="range"
            min={0}
            max={frames.length - 1}
            value={index}
            onChange={(e) => {
              setPlaying(false);
              setIndex(Number(e.target.value));
            }}
            aria-label="Timelapse position"
            aria-valuetext={`${clock(cur.capturedAt)}, frame ${index + 1} of ${frames.length}`}
            className="h-5 w-full accent-foreground"
          />
          <div className="flex justify-between font-mono text-axis text-subtle-foreground tnum" aria-hidden="true">
            <span>{clock(frames[0].capturedAt)}</span>
            <span>
              {index + 1} / {frames.length}
            </span>
            <span>{clock(frames[frames.length - 1].capturedAt)}</span>
          </div>
        </div>
        <ToggleGroup type="single" value={String(speed)} onValueChange={changeSpeed} aria-label="Playback speed" variant="outline" size="sm" spacing={0}>
          {SPEEDS.map((s) => (
            <ToggleGroupItem key={s} value={String(s)} aria-label={`${s} times real time`} className="font-mono text-caption">
              {s}×
            </ToggleGroupItem>
          ))}
        </ToggleGroup>
      </div>

      {showStrip && (
        <ul
          ref={stripRef}
          data-strip=""
          aria-label="Screenshots, arrow keys move"
          className="grid grid-cols-6 gap-1.5 @container"
          onKeyDown={(e) => {
            const items = Array.from(stripRef.current?.querySelectorAll<HTMLButtonElement>("button[data-strip-item]") ?? []);
            const at = items.indexOf(e.target as HTMLButtonElement);
            if (at < 0) return;
            const to = e.key === "ArrowRight" ? at + 1 : e.key === "ArrowLeft" ? at - 1 : e.key === "Home" ? 0 : e.key === "End" ? items.length - 1 : null;
            if (to === null || to < 0 || to >= items.length) return;
            e.preventDefault();
            setPlaying(false);
            setIndex(frames.indexOf(strip[to]));
            items[to].focus();
          }}
        >
          {strip.map((s, n) => {
            const i = frames.indexOf(s);
            const t = cachedThumb(s.id);
            const on = i === index;
            return (
              <li key={s.id} className="min-w-0">
                <button
                  type="button"
                  data-strip-item=""
                  tabIndex={n === stripStop ? 0 : -1}
                  onClick={() => {
                    setPlaying(false);
                    setIndex(i);
                  }}
                  aria-label={`Go to ${clock(s.capturedAt)}${s.hiddenApp ? `, ${s.hiddenApp} was in front` : ""}`}
                  aria-current={on ? "true" : undefined}
                  title={s.hiddenApp ? `${s.hiddenApp} was in front; not captured` : (s.windowTitle ?? undefined)}
                  className={cn(
                    "relative block aspect-video w-full overflow-hidden rounded-sm border bg-surface-2 transition-[border-color] duration-(--dur-fast) hover:border-input",
                    on && "outline-2 outline-offset-1 outline-foreground",
                    !isFrame(s) && "hatch-gap border-dashed",
                  )}
                >
                  {!isFrame(s) ? (
                    <EyeOff className="absolute inset-0 m-auto size-3.5 text-muted-foreground" aria-hidden="true" />
                  ) : t ? (
                    <img src={t} alt="" className="block h-full w-full object-cover" draggable={false} />
                  ) : null}
                </button>
                <span className="mt-0.5 block truncate font-mono text-axis text-subtle-foreground tnum">{clock(s.capturedAt)}</span>
              </li>
            );
          })}
        </ul>
      )}
      <p className="text-caption text-muted-foreground">
        {frames.length} {frames.length === 1 ? "screenshot" : "screenshots"} over {formatDuration(card.endTs - card.startTs)}; plays in about{" "}
        {seconds < 90 ? `${Math.max(1, Math.round(seconds))} s` : formatDuration(seconds)} at {speed}×. Space plays and pauses{arrowKeys ? ", ← and → step" : ""}.
      </p>

      <ShotViewer
        card={card}
        frames={frames}
        index={viewer}
        onIndex={(i) => {
          setViewer(i);
          if (i !== null) setIndex(i);
        }}
      />
    </section>
  );
}
