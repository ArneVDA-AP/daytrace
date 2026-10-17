import { Bell, Bot, Cpu, EyeOff, FileText, HardDrive, Languages, Palette, RefreshCw, Shapes, Video, type LucideIcon } from "lucide-react";

/** Settings pages in list order, addressable as #/settings?section=<id>. No components here, so
 *  the command menu can list them without loading the Settings code. */
export interface SettingsSectionMeta {
  id: string;
  label: string;
  icon: LucideIcon;
  /** Extra words the command menu matches on. */
  keywords: string;
}

export const SETTINGS_SECTION_META: SettingsSectionMeta[] = [
  { id: "provider", label: "Model provider", icon: Cpu, keywords: "api key claude ollama lm studio model test connection fallback" },
  { id: "prompts", label: "Language and prompts", icon: Languages, keywords: "language prompt instructions" },
  { id: "recording", label: "Recording", icon: Video, keywords: "interval screenshot size away pause start with windows retention" },
  { id: "privacy", label: "Private apps", icon: EyeOff, keywords: "privacy blocked apps password manager" },
  { id: "categories", label: "Categories", icon: Shapes, keywords: "colours colors work personal distraction idle counts as focus neither" },
  { id: "recap", label: "Daily recap", icon: FileText, keywords: "standup export folder automatic recap model" },
  { id: "storage", label: "Storage", icon: HardDrive, keywords: "disk space delete screenshots backup data folder" },
  { id: "ai-tools", label: "AI tools", icon: Bot, keywords: "mcp claude code desktop codex cursor vs code vscode" },
  { id: "notifications", label: "Notifications", icon: Bell, keywords: "toast alerts daily goal prompt" },
  { id: "appearance", label: "Appearance", icon: Palette, keywords: "theme light dark icons favicon app site" },
  { id: "about", label: "Updates and about", icon: RefreshCw, keywords: "version update licence license credits" },
];
