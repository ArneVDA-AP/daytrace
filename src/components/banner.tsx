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
 * plain words; the body says what Daytrace does by itself and what the button does.
 */
export function Banner({
  tone,
  icon,
  title,
  actions,
  children,
  className,
}: VariantProps<typeof bannerVariants> & {
  icon?: React.ReactNode;
  title: React.ReactNode;
  actions?: React.ReactNode;
  children?: React.ReactNode;
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
        {children && <p className="mt-0.5 text-meta text-pretty">{children}</p>}
      </div>
      {actions && <div className="flex shrink-0 flex-wrap items-center gap-2 self-center">{actions}</div>}
    </div>
  );
}
