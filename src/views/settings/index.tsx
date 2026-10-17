import * as React from "react";
import { Bell, Bot, CircleAlert, CircleCheck, Cpu, EyeOff, FileText, HardDrive, Languages, Palette, RefreshCw, Shapes, Video, type LucideIcon } from "lucide-react";
import { PageHeader } from "@/components/page-header";
import { Spinner } from "@/components/ui/spinner";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { href, useRoute } from "@/lib/router";
import { cn } from "@/lib/utils";
import { AiToolsSection } from "./ai-tools-section";
import { CategoriesSection } from "./categories-section";
import { AboutSection, AppearanceSection, NotificationsSection } from "./general-sections";
import { PrivateAppsSection } from "./private-apps";
import { PromptsSection } from "./prompts-section";
import { ProviderSection } from "./provider-section";
import { RecapSection } from "./recap-section";
import { RecordingSection } from "./recording-section";
import { SaveStatusProvider, useSaveState, useSaveStatusValue } from "./save";
import { StorageSection } from "./storage-section";

/*
 * Settings: one page per section, picked in the list on the left (icons only below 960 px) and
 * addressable as #/settings?section=storage. Every control applies as you change it; text fields
 * save when you leave them, and when you leave the page (see save.ts).
 */

interface SettingsPage {
  id: string;
  label: string;
  icon: LucideIcon;
  Component: React.ComponentType;
}

export const SETTINGS_SECTIONS: SettingsPage[] = [
  { id: "provider", label: "Model provider", icon: Cpu, Component: ProviderSection },
  { id: "prompts", label: "Language and prompts", icon: Languages, Component: PromptsSection },
  { id: "recording", label: "Recording", icon: Video, Component: RecordingSection },
  { id: "privacy", label: "Private apps", icon: EyeOff, Component: PrivateAppsSection },
  { id: "categories", label: "Categories", icon: Shapes, Component: CategoriesSection },
  { id: "recap", label: "Daily recap", icon: FileText, Component: RecapSection },
  { id: "storage", label: "Storage", icon: HardDrive, Component: StorageSection },
  { id: "ai-tools", label: "AI tools", icon: Bot, Component: AiToolsSection },
  { id: "notifications", label: "Notifications", icon: Bell, Component: NotificationsSection },
  { id: "appearance", label: "Appearance", icon: Palette, Component: AppearanceSection },
  { id: "about", label: "Updates and about", icon: RefreshCw, Component: AboutSection },
];

export default function SettingsSection() {
  const { params } = useRoute();
  const page = SETTINGS_SECTIONS.find((p) => p.id === params.get("section")) ?? SETTINGS_SECTIONS[0];
  const status = useSaveStatusValue();
  const scroller = React.useRef<HTMLDivElement>(null);

  // A new section starts at its top.
  React.useEffect(() => {
    scroller.current?.scrollTo({ top: 0 });
  }, [page.id]);

  const Page = page.Component;
  return (
    <SaveStatusProvider value={status}>
      <PageHeader label="Settings" title={page.label} actions={<SaveStatusText />} />
      <div className="flex min-h-0 flex-1">
        <nav aria-label="Settings sections" className="scroll-thin w-56 shrink-0 overflow-y-auto border-r p-2 max-split:w-14">
          <ul className="flex flex-col gap-0.5">
            {SETTINGS_SECTIONS.map((p) => (
              <li key={p.id}>
                <SectionLink page={p} active={p.id === page.id} />
              </li>
            ))}
          </ul>
        </nav>
        <div ref={scroller} className="scroll-thin min-h-0 min-w-0 flex-1 overflow-y-auto">
          <div className="mx-auto flex w-full max-w-3xl flex-col gap-6 px-6 pt-5 pb-10 max-split:px-5">
            <Page key={page.id} />
          </div>
        </div>
      </div>
    </SaveStatusProvider>
  );
}

function SectionLink({ page, active }: { page: SettingsPage; active: boolean }) {
  const Icon = page.icon;
  const link = (
    <a
      href={href("settings", { section: page.id })}
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex h-8 items-center gap-2.5 rounded-md px-2.5 text-meta text-muted-foreground hover:bg-surface-2 hover:text-foreground max-split:justify-center max-split:px-0",
        active && "bg-surface-2 font-medium text-foreground",
      )}
    >
      <Icon className="size-4 shrink-0" aria-hidden />
      <span className="truncate max-split:sr-only">{page.label}</span>
    </a>
  );
  return <NarrowTooltip label={page.label}>{link}</NarrowTooltip>;
}

/** Below 960 px the section list shows icons only; the tooltip names them. */
function NarrowTooltip({ label, children }: { label: string; children: React.ReactElement }) {
  const [narrow, setNarrow] = React.useState(() => !matchMedia("(min-width: 60rem)").matches);
  React.useEffect(() => {
    const mq = matchMedia("(min-width: 60rem)");
    const on = () => setNarrow(!mq.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, []);
  if (!narrow) return children;
  return (
    <Tooltip>
      <TooltipTrigger asChild>{children}</TooltipTrigger>
      <TooltipContent side="right">{label}</TooltipContent>
    </Tooltip>
  );
}

/** "Changes save as you make them" / Saving… / Saved / Couldn't save, in the header. */
function SaveStatusText() {
  const state = useSaveState();
  return (
    <p role="status" className="flex items-center gap-1.5 text-caption text-muted-foreground">
      {state.kind === "saving" && (
        <>
          <Spinner className="size-3.5" aria-hidden role={undefined} aria-label={undefined} /> Saving…
        </>
      )}
      {state.kind === "saved" && (
        <>
          <CircleCheck className="size-3.5 text-live-foreground" aria-hidden /> Saved
        </>
      )}
      {state.kind === "error" && (
        <span className="flex items-center gap-1.5 text-destructive">
          <CircleAlert className="size-3.5" aria-hidden /> Couldn’t save. The message is next to the setting.
        </span>
      )}
      {state.kind === "idle" && "Changes save as you make them."}
    </p>
  );
}
