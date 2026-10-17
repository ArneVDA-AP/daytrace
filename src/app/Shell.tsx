import * as React from "react";
import { CalendarRange, Clock3, Goal, Laptop, ListChecks, MessagesSquare, Moon, Search, Settings, Sun, type LucideIcon } from "lucide-react";
import { useRecapBadge } from "@/app/badges";
import { useDay, isDaySection } from "@/app/day";
import { LogoMark } from "@/app/Logo";
import { useAppSettings } from "@/app/state";
import { StatusBlock } from "@/app/StatusBlock";
import { Kbd } from "@/components/ui/kbd";
import { toast } from "@/components/ui/toast";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { href, useRoute, type Section } from "@/lib/router";
import type { Appearance } from "@/lib/theme";
import { cn, errorText } from "@/lib/utils";

export interface NavItem {
  id: Section;
  label: string;
  icon: LucideIcon;
  /** Ctrl+<key>: a digit (matched by physical key, lib/keys.ts) or "," for Settings. */
  key: string;
}

/**
 * The rail's sections registry, in rail order: one line per section. Goals is the "journal" view
 * (Dayflow's day goals). Ctrl+1… follow the order.
 */
export const NAV: NavItem[] = [
  { id: "timeline", label: "Timeline", icon: Clock3, key: "1" },
  { id: "daily", label: "Daily", icon: ListChecks, key: "2" },
  { id: "weekly", label: "Weekly", icon: CalendarRange, key: "3" },
  { id: "journal", label: "Goals", icon: Goal, key: "4" },
  { id: "chat", label: "Chat", icon: MessagesSquare, key: "5" },
];

export const SETTINGS_NAV: NavItem = { id: "settings", label: "Settings", icon: Settings, key: "," };

export const SECTION_TITLES: Record<Section, string> = {
  timeline: "Timeline",
  daily: "Daily",
  weekly: "Weekly",
  journal: "Goals",
  chat: "Chat",
  settings: "Settings",
};

const WIDE_QUERY = "(min-width: 68.75rem)";

/** True when the rail is collapsed to icons (window narrower than 1100 px). */
function useCollapsedRail(): boolean {
  const [collapsed, setCollapsed] = React.useState(() => !matchMedia(WIDE_QUERY).matches);
  React.useEffect(() => {
    const mq = matchMedia(WIDE_QUERY);
    const on = () => setCollapsed(!mq.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, []);
  return collapsed;
}

/** Tooltips matter only when the rail is collapsed; the expanded rail shows the label itself. */
function RailTooltip({ label, collapsed, children }: { label: React.ReactNode; collapsed: boolean; children: React.ReactElement }) {
  if (!collapsed) return children;
  return (
    <Tooltip>
      <TooltipTrigger asChild>{children}</TooltipTrigger>
      <TooltipContent side="right">{label}</TooltipContent>
    </Tooltip>
  );
}

/** Link to a section. Day sections carry the shared day so it travels with you. */
export function sectionHref(id: Section, day: string): string {
  return href(id, isDaySection(id) ? { day } : {});
}

function NavLink({ item, active, collapsed, badge }: { item: NavItem; active: boolean; collapsed: boolean; badge?: string }) {
  const { day } = useDay();
  const Icon = item.icon;
  const shortcut = `Ctrl+${item.key}`;
  return (
    <RailTooltip
      collapsed={collapsed}
      label={
        <>
          {item.label}
          {badge && <span className="text-muted-foreground"> · {badge}</span>} <Kbd className="ml-1">{shortcut}</Kbd>
        </>
      }
    >
      <a
        href={sectionHref(item.id, day)}
        data-rail-link=""
        aria-current={active ? "page" : undefined}
        aria-keyshortcuts={`Control+${item.key === "," ? "Comma" : item.key}`}
        aria-label={collapsed ? (badge ? `${item.label}, ${badge.toLowerCase()}` : item.label) : undefined}
        className={cn(
          "group relative flex h-9 items-center gap-3 rounded-md px-2.5 text-body text-muted-foreground transition-[background-color,color] duration-(--dur-fast) hover:bg-surface-2 hover:text-foreground max-wide:justify-center max-wide:px-0",
          active && "bg-surface-1 font-semibold text-foreground shadow-xs ring-1 ring-border hover:bg-surface-1",
        )}
      >
        {/* The selected marker. In Contrast themes backgrounds give way to the system's, so it is
            drawn in Highlight there (UX6). */}
        {active && (
          <span
            aria-hidden="true"
            className="absolute top-2 bottom-2 left-0 w-[3px] rounded-full bg-foreground forced-colors:top-1 forced-colors:bottom-1 forced-colors:w-1 forced-colors:bg-[Highlight] forced-colors:forced-color-adjust-none"
          />
        )}
        <span className="relative shrink-0">
          <Icon className="size-[1.125rem]" aria-hidden="true" />
          {badge && (
            <span
              aria-hidden="true"
              data-mark=""
              className="absolute -top-1 -right-1 size-2 rounded-full bg-primary ring-2 ring-background wide:hidden forced-colors:bg-[CanvasText]"
            />
          )}
        </span>
        <span className="hidden flex-1 wide:inline">
          {item.label}
          {badge && <span className="sr-only">, {badge.toLowerCase()}</span>}
        </span>
        {badge && (
          <span aria-hidden="true" data-mark="" className="hidden size-2 shrink-0 rounded-full bg-primary group-hover:hidden group-focus-visible:hidden wide:inline-block forced-colors:bg-[CanvasText]" />
        )}
        <span
          className={cn(
            "hidden text-caption font-normal text-subtle-foreground opacity-0 group-hover:opacity-100 group-focus-visible:opacity-100 wide:inline",
            badge && "wide:hidden wide:group-hover:inline wide:group-focus-visible:inline",
          )}
        >
          {shortcut}
        </span>
      </a>
    </RailTooltip>
  );
}

function SearchButton({ onClick, collapsed }: { onClick: () => void; collapsed: boolean }) {
  return (
    <RailTooltip
      collapsed={collapsed}
      label={
        <>
          Jump to… <Kbd className="ml-1">Ctrl+K</Kbd>
        </>
      }
    >
      <button
        type="button"
        onClick={onClick}
        aria-label="Jump to…"
        aria-keyshortcuts="Control+K"
        className="flex h-8 w-full items-center gap-2 rounded-md border border-border-strong bg-surface-1 px-2 text-meta text-muted-foreground shadow-xs hover:border-input hover:text-foreground max-wide:justify-center max-wide:px-0"
      >
        <Search className="size-4 shrink-0" aria-hidden="true" />
        <span className="hidden flex-1 text-left wide:inline">Jump to…</span>
        <Kbd className="hidden wide:inline-flex">Ctrl K</Kbd>
      </button>
    </RailTooltip>
  );
}

const THEMES: { v: Appearance; label: string; icon: LucideIcon }[] = [
  { v: "system", label: "Follow Windows", icon: Laptop },
  { v: "light", label: "Light", icon: Sun },
  { v: "dark", label: "Dark", icon: Moon },
];

function ThemeSwitch({ collapsed }: { collapsed: boolean }) {
  const app = useAppSettings();
  const choice = app.settings.settings.appearance;
  const set = (a: Appearance) => {
    app.saveAppearance(a).catch((e) => toast("Couldn’t save the theme", { description: errorText(e), tone: "error" }));
  };
  const current = THEMES.find((t) => t.v === choice) ?? THEMES[0];
  if (collapsed) {
    const next = THEMES[(THEMES.indexOf(current) + 1) % THEMES.length];
    const Icon = current.icon;
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <button
            type="button"
            onClick={() => set(next.v)}
            aria-label={`Theme: ${current.label}. Switch to ${next.label}`}
            className="grid h-9 place-items-center rounded-md text-muted-foreground hover:bg-surface-2 hover:text-foreground"
          >
            <Icon className="size-4" aria-hidden="true" />
          </button>
        </TooltipTrigger>
        <TooltipContent side="right">Theme: {current.label}</TooltipContent>
      </Tooltip>
    );
  }
  return (
    <div role="radiogroup" aria-label="Theme" className="mt-1 flex h-8 items-center gap-0.5 rounded-md bg-surface-2 p-0.5">
      {THEMES.map((t) => {
        const Icon = t.icon;
        const on = choice === t.v;
        return (
          <Tooltip key={t.v}>
            <TooltipTrigger asChild>
              <button
                type="button"
                role="radio"
                data-choice=""
                aria-checked={on}
                aria-label={t.label}
                tabIndex={on ? 0 : -1}
                onClick={() => set(t.v)}
                onKeyDown={(e) => {
                  const d = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
                  if (!d) return;
                  e.preventDefault();
                  const next = THEMES[(THEMES.indexOf(t) + d + THEMES.length) % THEMES.length];
                  set(next.v);
                  (e.currentTarget.parentElement?.querySelector(`[aria-label="${next.label}"]`) as HTMLElement | null)?.focus();
                }}
                className={cn(
                  "grid h-7 flex-1 place-items-center rounded-[5px] text-muted-foreground hover:text-foreground",
                  on && "bg-surface-1 text-foreground shadow-xs ring-1 ring-border-strong",
                )}
              >
                <Icon className="size-4" aria-hidden="true" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="top">{t.label}</TooltipContent>
          </Tooltip>
        );
      })}
    </div>
  );
}

/**
 * Windows NavigationView-style shell: the rail sits on the window background, the section on a
 * raised content layer with an 8 px corner. Labels from 1100 px, icons with tooltips below.
 */
export function Shell({ children, onOpenCommand }: { children: React.ReactNode; onOpenCommand: () => void }) {
  const { section } = useRoute();
  const collapsed = useCollapsedRail();
  const recapNew = useRecapBadge(section);
  return (
    <div className="flex h-full min-w-0 bg-background">
      <a
        href="#content"
        onClick={(e) => {
          e.preventDefault();
          document.getElementById("content")?.focus();
        }}
        className="sr-only z-50 rounded-md bg-primary px-3 py-2 text-primary-foreground focus:not-sr-only focus:fixed focus:top-2 focus:left-2"
      >
        Skip to content
      </a>
      {/* The rail, one landmark: search, the section links, the status block, Settings and the
          theme switch. The section and Settings links are navigation landmarks inside it. */}
      <aside
        aria-label="Daytrace"
        className="scroll-thin flex w-(--rail-w-collapsed) shrink-0 flex-col gap-1 overflow-y-auto px-2 pt-3 pb-2 wide:w-(--rail-w) wide:px-3"
      >
        <div className="mb-3 flex h-8 items-center gap-2.5 px-1.5 max-wide:justify-center max-wide:px-0">
          <LogoMark className="size-6 shrink-0" />
          <span className="hidden font-display text-[1.0625rem] font-semibold tracking-[-0.01em] wide:inline" translate="no">
            Daytrace
          </span>
        </div>
        <SearchButton onClick={onOpenCommand} collapsed={collapsed} />
        <nav aria-label="Sections" className="mt-2 flex flex-col gap-0.5">
          {NAV.map((n) => (
            <NavLink key={n.id} item={n} active={section === n.id} collapsed={collapsed} badge={n.id === "daily" && recapNew ? "New recap" : undefined} />
          ))}
        </nav>
        <div className="min-h-4 flex-1" />
        <StatusBlock />
        <nav aria-label="Settings" className="flex flex-col">
          <NavLink item={SETTINGS_NAV} active={section === "settings"} collapsed={collapsed} />
        </nav>
        <ThemeSwitch collapsed={collapsed} />
      </aside>
      <div className="flex min-w-0 flex-1 flex-col py-2 pr-2">
        <main
          id="content"
          tabIndex={-1}
          aria-label={SECTION_TITLES[section]}
          className="relative flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden rounded-lg border bg-surface-1 shadow-xs outline-none"
        >
          {children}
        </main>
      </div>
    </div>
  );
}
