import * as React from "react";
import { GlobalBanners } from "@/app/Banners";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

/** "Today" / "This week" from DayNav, drawn right of the title (UX16: the title never moves). */
export interface JumpBack {
  label: string;
  go: () => void;
}

/** DayNav hands its jump-back button to the page header through this. */
export const JumpBackSlot = React.createContext<((j: JumpBack | null) => void) | null>(null);

/**
 * The header row every section starts with: optional navigation (DayNav), a small section label
 * above the one h1 of the screen, optional extra (a badge), and actions on the right. The global
 * problem banners follow directly below it, so every section shows them in the same place.
 */
export function PageHeader({
  label,
  title,
  nav,
  badge,
  actions,
  banners = true,
  className,
}: {
  /** Section name, shown small above the title ("Timeline"). */
  label?: React.ReactNode;
  /** The h1: a day ("Friday 16 October"), a week, or the section name. */
  title: React.ReactNode;
  nav?: React.ReactNode;
  /** Shown after the title, e.g. a "Today" badge. */
  badge?: React.ReactNode;
  actions?: React.ReactNode;
  /** Set false only where the banners' fixes live (none today). */
  banners?: boolean;
  className?: string;
}) {
  const [jump, setJump] = React.useState<JumpBack | null>(null);
  return (
    <>
      <header className={cn("flex min-h-14 shrink-0 flex-wrap items-center gap-x-4 gap-y-2 border-b px-(--page-x) py-2.5", className)}>
        <div className="flex min-w-0 items-center gap-2">
          <JumpBackSlot.Provider value={setJump}>{nav}</JumpBackSlot.Provider>
          <div className="min-w-0">
            {label && <p className="text-caption text-muted-foreground">{label}</p>}
            <h1 className="truncate font-display text-title font-semibold tracking-[-0.01em]">{title}</h1>
          </div>
          {badge}
          {jump && (
            <Button variant="outline" size="xs" className="mt-4 text-caption" onClick={jump.go}>
              {jump.label}
            </Button>
          )}
        </div>
        {actions && <div className="ml-auto flex flex-wrap items-center gap-2">{actions}</div>}
      </header>
      {banners && <GlobalBanners />}
    </>
  );
}

/** Small neutral pill after a title ("Today"). */
export function TitleBadge({ children }: { children: React.ReactNode }) {
  return (
    <span className="mt-4 inline-flex h-5 shrink-0 items-center rounded-sm bg-surface-2 px-1.5 text-caption text-muted-foreground">{children}</span>
  );
}
