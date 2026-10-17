/**
 * Daytrace mark: a timing-diagram trace (low, high, low, high) on an ink tile. It reads as "a
 * signal over time" and stays legible at 16 px (tray, taskbar). Monochrome on purpose: hue in this
 * product belongs to the categories.
 */
export function LogoMark({ className = "size-6" }: { className?: string }) {
  return (
    <svg viewBox="0 0 24 24" className={className} aria-hidden="true" focusable="false">
      <rect x="0" y="0" width="24" height="24" rx="6" fill="var(--foreground)" />
      <path
        d="M4 15.5h3.5V8.5h4v7h3V8.5H20"
        fill="none"
        stroke="var(--background)"
        strokeWidth="2.2"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
