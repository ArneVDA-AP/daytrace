import * as React from "react";
import { CircleAlert, CircleCheck } from "lucide-react";
import { Banner } from "@/components/banner";
import { NativeSelect, NativeSelectOption } from "@/components/ui/native-select";
import { cn } from "@/lib/utils";

/*
 * Building blocks of the settings pages (Windows Settings rows): a group is a titled card of rows;
 * a row has its label and explanation on the left and the control on the right, or the control
 * below the label (`stack`) when it needs the width.
 */

export function Group({
  title,
  description,
  children,
  actions,
  className,
}: {
  title?: string;
  description?: React.ReactNode;
  children: React.ReactNode;
  /** Buttons after the title (e.g. "Reset all to default"). */
  actions?: React.ReactNode;
  className?: string;
}) {
  const id = React.useId();
  return (
    <section aria-labelledby={title ? id : undefined} className={cn("flex flex-col gap-2", className)}>
      {title && (
        <div className="flex flex-wrap items-end gap-x-4 gap-y-1">
          <div className="min-w-0 flex-1 basis-72">
            <h2 id={id} className="text-body font-semibold">
              {title}
            </h2>
            {description && <p className="mt-0.5 max-w-[72ch] text-meta text-muted-foreground">{description}</p>}
          </div>
          {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
        </div>
      )}
      <div className="flex flex-col divide-y rounded-lg border bg-surface-1">{children}</div>
    </section>
  );
}

export function Row({
  label,
  description,
  htmlFor,
  labelId,
  children,
  stack,
  id,
  className,
}: {
  label: React.ReactNode;
  description?: React.ReactNode;
  /** The control the label names (a <label for>). */
  htmlFor?: string;
  /** For a group of controls: the id the group's aria-labelledby points to. */
  labelId?: string;
  children?: React.ReactNode;
  /** Control under the label, full width. */
  stack?: boolean;
  id?: string;
  className?: string;
}) {
  const Tag = htmlFor ? "label" : "p";
  return (
    <div id={id} className={cn("flex scroll-mt-4 gap-x-6 gap-y-2.5 px-4 py-3", stack ? "flex-col" : "flex-wrap items-center", className)}>
      <div className={cn("min-w-0", !stack && "flex-1 basis-64")}>
        <Tag id={labelId} htmlFor={htmlFor} className="block text-body font-medium">
          {label}
        </Tag>
        {description && <div className="mt-0.5 max-w-[68ch] text-meta text-muted-foreground [&_p+p]:mt-1">{description}</div>}
      </div>
      {children && <div className={cn("flex min-w-0 shrink-0 flex-wrap items-center gap-2", stack && "w-full flex-col items-stretch")}>{children}</div>}
    </div>
  );
}

export interface Option<T extends string | number> {
  value: T;
  label: string;
}

/** A native <select> (Windows draws the list). Adds the current value when it is not an option. */
export function Choice<T extends string | number>({
  id,
  value,
  options,
  onChange,
  className,
  disabled,
  "aria-label": ariaLabel,
  "aria-describedby": describedBy,
  fallbackLabel,
}: {
  id?: string;
  value: T;
  options: Option<T>[];
  onChange: (v: T) => void;
  className?: string;
  disabled?: boolean;
  "aria-label"?: string;
  "aria-describedby"?: string;
  /** Label for a stored value that is not in `options`. */
  fallbackLabel?: (v: T) => string;
}) {
  const known = options.some((o) => o.value === value);
  const list = known ? options : [...options, { value, label: fallbackLabel ? fallbackLabel(value) : String(value) }];
  return (
    <NativeSelect
      id={id}
      value={String(value)}
      disabled={disabled}
      aria-label={ariaLabel}
      aria-describedby={describedBy}
      className={className}
      onChange={(e) => {
        const hit = list.find((o) => String(o.value) === e.target.value);
        if (hit) onChange(hit.value);
      }}
    >
      {list.map((o) => (
        <NativeSelectOption key={String(o.value)} value={String(o.value)}>
          {o.label}
        </NativeSelectOption>
      ))}
    </NativeSelect>
  );
}

/** A validation message or a refused save, under the field it belongs to. */
export function FieldError({ id, children }: { id?: string; children: React.ReactNode }) {
  if (!children) return null;
  return (
    <p id={id} className="flex items-start gap-1.5 text-meta text-destructive">
      <CircleAlert className="mt-0.5 size-3.5 shrink-0" aria-hidden />
      <span className="min-w-0 break-words">{children}</span>
    </p>
  );
}

export interface Outcome {
  ok: boolean;
  text: string;
}

/**
 * The result of an action started in a row (Test connection, Find claude, Back up now): one line
 * with a check when it worked, a framed message when it did not. Put it inside an aria-live region.
 */
export function OutcomeLine({ outcome, className }: { outcome: Outcome | null; className?: string }) {
  if (!outcome) return null;
  if (outcome.ok) {
    return (
      <p className={cn("flex items-start gap-2 text-meta", className)}>
        <CircleCheck className="mt-0.5 size-4 shrink-0 text-live-foreground" aria-hidden />
        <span className="min-w-0 break-words">{outcome.text}</span>
      </p>
    );
  }
  return (
    <div className={cn("flex items-start gap-2 rounded-md border border-destructive/40 bg-destructive-surface px-3 py-2 text-meta", className)}>
      <CircleAlert className="mt-0.5 size-4 shrink-0 text-destructive" aria-hidden />
      <p className="min-w-0 break-words whitespace-pre-line">{outcome.text}</p>
    </div>
  );
}

/** An error that belongs to this page: what failed, the fix as a button, the raw message folded. */
export function ErrorBanner({ title, message, action, children }: { title: string; message: string; action?: React.ReactNode; children?: React.ReactNode }) {
  const [open, setOpen] = React.useState(false);
  const id = React.useId();
  // Banner puts its body in a <p>, so the disclosure is a button and a span (phrasing content).
  return (
    <Banner tone="error" icon={<CircleAlert aria-hidden />} title={title} actions={action}>
      {children}
      {children && " "}
      <button
        type="button"
        aria-expanded={open}
        aria-controls={id}
        onClick={() => setOpen((o) => !o)}
        className="rounded-sm text-meta text-muted-foreground underline underline-offset-2 hover:text-foreground"
      >
        {open ? "Hide error details" : "Error details"}
      </button>
      <span id={id} hidden={!open} className="mt-1 block font-mono text-caption break-words whitespace-pre-wrap">
        {message}
      </span>
    </Banner>
  );
}

/** Plain explanatory text between groups. */
export function Note({ children, className }: { children: React.ReactNode; className?: string }) {
  return <p className={cn("max-w-[72ch] text-meta text-muted-foreground", className)}>{children}</p>;
}
