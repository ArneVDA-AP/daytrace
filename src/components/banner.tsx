import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const bannerVariants = cva("flex flex-wrap items-start gap-x-3 gap-y-2 rounded-lg border px-3.5 py-3 text-foreground", {
  variants: {
    tone: {
      error: "border-destructive/40 bg-destructive-surface [&>[data-slot=banner-icon]]:text-destructive",
      warn: "border-warn/50 bg-warn-surface [&>[data-slot=banner-icon]]:text-warn-foreground",
      info: "border-border-strong bg-info-surface [&>[data-slot=banner-icon]]:text-muted-foreground",
    },
  },
  defaultVariants: { tone: "info" },
});

/**
 * A message that needs attention, with its fix as a button. role="alert" for error and warn
 * (announced once when it appears), role="status" for info. The title says what is wrong in
 * plain words; the body says what Daytrace does by itself and what the button does. `details`
 * (the raw error) goes in a collapsed "Error details" disclosure under the body.
 */
export function Banner({
  tone,
  icon,
  title,
  actions,
  children,
  details,
  className,
}: VariantProps<typeof bannerVariants> & {
  icon?: React.ReactNode;
  title: React.ReactNode;
  actions?: React.ReactNode;
  children?: React.ReactNode;
  /** Technical text (the backend's message), shown on request. */
  details?: string | null;
  className?: string;
}) {
  return (
    <div role={tone === "info" || !tone ? "status" : "alert"} data-slot="banner" className={cn(bannerVariants({ tone }), className)}>
      {icon && (
        <span data-slot="banner-icon" className="mt-0.5 shrink-0 [&_svg]:size-4">
          {icon}
        </span>
      )}
      <div className="min-w-0 flex-1 basis-80">
        <p className="font-semibold">{title}</p>
        {children && <div className="mt-0.5 text-meta text-pretty">{children}</div>}
        {details && (
          <details className="mt-1.5 text-caption">
            <summary className="w-fit cursor-default rounded-sm text-muted-foreground select-none hover:text-foreground">Error details</summary>
            <pre className="scroll-thin mt-1.5 max-h-48 overflow-auto rounded-md border bg-surface-1 p-2.5 font-mono text-caption whitespace-pre-wrap [overflow-wrap:anywhere]">
              {details}
            </pre>
          </details>
        )}
      </div>
      {actions && <div className="flex shrink-0 flex-wrap items-center gap-2 self-center">{actions}</div>}
    </div>
  );
}
