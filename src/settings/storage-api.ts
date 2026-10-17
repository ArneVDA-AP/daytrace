import { invoke } from "@tauri-apps/api/core";

// Types mirror src-tauri/src/features/storage.rs and storage/backup.rs (serde camelCase).

export type HealthStatus = "unchecked" | "ok" | "restored" | "reset" | "damaged";

export interface DbHealth {
  status: HealthStatus;
  detail: string | null;
  restoredFrom: string | null;
  restoredBackupAt: number | null;
  movedTo: string | null;
  checkedAt: number;
}

export interface BackupInfo {
  name: string;
  bytes: number;
  createdAt: number;
}

export interface DayUsage {
  day: string;
  screenshots: number;
  bytes: number;
}

export interface StorageUsage {
  dataDir: string;
  screenshotsBytes: number;
  screenshotCount: number;
  days: DayUsage[];
  databaseBytes: number;
  backupsBytes: number;
  backups: BackupInfo[];
  logsBytes: number;
  bytesPerHour: number | null;
  averageDayBytes: number | null;
  health: DbHealth;
}

export interface PurgeResult {
  filesRemoved: number;
  bytesFreed: number;
}

export interface AgentSetup {
  exePath: string;
  installed: boolean;
  claudeCodeCommand: string;
  desktopConfig: string;
  desktopConfigPath: string;
}

export const storageApi = {
  usage: () => invoke<StorageUsage>("storage_usage"),
  deleteOlderThan: (days: number) => invoke<PurgeResult>("storage_delete_older_than", { days }),
  deleteDay: (day: string) => invoke<PurgeResult>("storage_delete_day", { day }),
  backupNow: () => invoke<BackupInfo>("storage_backup_now"),
  health: () => invoke<DbHealth>("storage_health"),
  agentSetup: () => invoke<AgentSetup>("storage_agent_setup"),
};

/** 1 GB = 10^9 bytes, as Windows Explorer's size column and Dayflow's storage settings count. */
export const GB = 1_000_000_000;

export function formatBytes(n: number): string {
  if (n >= GB) return `${(n / GB).toFixed(n >= 10 * GB ? 0 : 1)} GB`;
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(n >= 10_000_000 ? 0 : 1)} MB`;
  if (n >= 1_000) return `${Math.round(n / 1_000)} KB`;
  return `${n} bytes`;
}

export function formatWhen(ts: number): string {
  return new Date(ts * 1000).toLocaleString("en-GB", { weekday: "short", day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" });
}
