import type * as React from "react";

/**
 * Interim container for views that still use the old stylesheets (styles.css and the per-feature
 * CSS files). Their rules and variables only apply inside it, and Tailwind's base does not reach
 * into it. Never put new components inside: --muted and --accent mean something else in here.
 * Delete the frame (and the view's legacy CSS) when the view is ported.
 */
export function LegacyFrame({ children }: { children: React.ReactNode }) {
  return <div className="legacy">{children}</div>;
}
