import type { Settings } from "../api";

/** Props for a settings section that edits fields of the shared Settings object. */
export interface SectionProps {
  s: Settings;
  set: <K extends keyof Settings>(k: K, v: Settings[K]) => void;
}
