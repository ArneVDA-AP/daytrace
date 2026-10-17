import * as React from "react";
import { CircleAlert, CircleCheck } from "lucide-react";
import { PageHeader } from "@/components/page-header";
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select";
import { Spinner } from "@/components/ui/spinner";
import { href, navigate, useRoute } from "@/lib/router";
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
import { SETTINGS_SECTION_META, type SettingsSectionMeta } from "./sections";
import { StorageSection } from "./storage-section";

/*
 * Settings: one page per section, picked in the list on the left (a "Section" list box above the
 * page below 960 px) and addressable as #/settings?section=storage. Every control applies as you
 * change it; text fields save when you leave them, and when you leave the page (see save.ts).
 */

interface SettingsPage extends SettingsSectionMeta {
  Component: React.ComponentType;
}

const COMPONENTS: Record<string, React.ComponentType> = {
  provider: ProviderSection,
  prompts: PromptsSection,
  recording: RecordingSection,
  privacy: PrivateAppsSection,
  categories: CategoriesSection,
  recap: RecapSection,
  storage: StorageSection,
  "ai-tools": AiToolsSection,
  notifications: NotificationsSection,
  appearance: AppearanceSection,
  about: AboutSection,
};

export const SETTINGS_SECTIONS: SettingsPage[] = SETTINGS_SECTION_META.map((m) => ({ ...m, Component: COMPONENTS[m.id] }));

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
        <nav aria-label="Settings sections" className="scroll-thin w-56 shrink-0 overflow-y-auto border-r p-2 max-split:hidden">
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
            <SectionPicker current={page.id} />
            <Page key={page.id} />
          </div>
        </div>
      </div>
    </SaveStatusProvider>
  );
}

function SectionLink({ page, active }: { page: SettingsPage; active: boolean }) {
  const Icon = page.icon;
  return (
    <a
      href={href("settings", { section: page.id })}
      aria-current={active ? "page" : undefined}
      className={cn(
        "flex h-8 items-center gap-2.5 rounded-md px-2.5 text-meta text-muted-foreground hover:bg-surface-2 hover:text-foreground",
        active && "bg-surface-2 font-medium text-foreground",
      )}
    >
      <Icon className="size-4 shrink-0" aria-hidden />
      <span className="truncate">{page.label}</span>
    </a>
  );
}

/** Below 960 px there is no room for the list of sections: a list box above the page picks one. */
function SectionPicker({ current }: { current: string }) {
  const id = React.useId();
  return (
    <div className="flex items-center gap-3 split:hidden">
      <label htmlFor={id} className="text-meta font-medium">
        Section
      </label>
      <NativeSelect
        id={id}
        value={current}
        wrapperClassName="min-w-0 flex-1"
        className="w-full"
        onChange={(e) => navigate("settings", { section: e.target.value }, { replace: true })}
      >
        {SETTINGS_SECTIONS.map((p) => (
          <NativeSelectOption key={p.id} value={p.id}>
            {p.label}
          </NativeSelectOption>
        ))}
      </NativeSelect>
    </div>
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
