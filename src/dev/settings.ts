/*
 * DEV ONLY: Settings and first-run setup fixtures for the browser preview (src/dev/mock-ipc.ts
 * merges these handlers over its own). SYNTHETIC data only.
 *
 * Variants, as `?settings=` before the hash (comma-separated):
 *   test-fail      Test connection is refused (401)
 *   models-fail    Show available models fails
 *   update         a new release is available
 *   damaged        the settings file could not be read (setup notice)
 *   restored | reset | db-damaged   database health notices on Storage
 *   storage-error  disk usage can't be read
 *   mcp-ok         daytrace-mcp is installed next to Daytrace
 *   fallback       a fallback provider (Ollama) is set
 *   custom         categories with changed colours and a fourth category
 *   nokeys         no API key stored for any provider (like ?status=nokey, for every slot)
 * Example: http://localhost:1420/?settings=test-fail#/settings?section=provider
 */
import type { ProviderKind, Settings, SettingsDto } from "@/api";
import type { DbHealth } from "@/views/settings/storage-api";

type Args = Record<string, unknown>;

/** Credential slots as config.rs key_slot names them. */
function slot(kind: ProviderKind, baseUrl: string): string | null {
  if (kind === "anthropic") return "anthropic";
  if (kind === "lm_studio") return "lm_studio";
  if (kind === "openai_compatible") {
    try {
      return `openai:${new URL(baseUrl.trim()).host.toLowerCase()}`;
    } catch {
      return null;
    }
  }
  return null;
}

/** Puts the settings handlers over mock-ipc's own (wrapping storage_usage and storage_agent_setup). */
export function applySettingsMock(
  base: Record<string, (a: Args) => unknown>,
  ctx: { q: URLSearchParams; flags: Set<string>; settings: Settings; now: () => number; dataDir: string },
) {
  const v = new Set((ctx.q.get("settings") ?? "").split(",").filter(Boolean));
  const s = ctx.settings;
  const keys = new Set<string>(ctx.flags.has("nokey") || v.has("nokeys") ? [] : ["anthropic", "openai:api.openai.com"]);
  const fallbackKeys = new Set<string>();

  if (v.has("fallback")) {
    s.fallbackProvider = "ollama";
    s.fallbackBaseUrl = "http://localhost:11434";
    s.fallbackModel = "qwen3-vl:4b";
  }
  if (v.has("custom")) {
    s.categories = s.categories.map((c) => (c.name === "Work" ? { ...c, color: "#2A78D6" } : c.name === "Distraction" ? { ...c, color: "#E34948" } : c));
    const idle = s.categories.findIndex((c) => c.isIdle);
    s.categories.splice(idle < 0 ? s.categories.length : idle, 0, { name: "Study", color: "#048F90", description: "Courses, reading and exercises for school.", isIdle: false });
  }

  const dto = (): SettingsDto => {
    const k = slot(s.provider, s.baseUrl);
    return { settings: structuredClone(s), hasApiKey: k !== null && keys.has(k), dataDir: ctx.dataDir };
  };

  const health = (): DbHealth => {
    const base: DbHealth = { status: "ok", detail: null, restoredFrom: null, restoredBackupAt: null, movedTo: null, checkedAt: ctx.now() - 600 };
    if (v.has("restored"))
      return { ...base, status: "restored", restoredFrom: "daytrace-backup-6.db", restoredBackupAt: ctx.now() - 26 * 3600, movedTo: "daytrace.sqlite.damaged-20261016" };
    if (v.has("reset")) return { ...base, status: "reset", movedTo: "daytrace.sqlite.damaged-20261016" };
    if (v.has("db-damaged")) return { ...base, status: "damaged", detail: "database disk image is malformed (code 11)" };
    return base;
  };

  const update = (checked: number) => ({
    currentVersion: "0.2.1",
    latestVersion: v.has("update") ? "0.3.0" : "0.2.1",
    releaseUrl: v.has("update") ? "https://github.com/example/daytrace/releases/tag/v0.3.0" : null,
    available: v.has("update"),
    noReleases: false,
    checkedAt: checked,
    error: null,
    attemptedAt: checked,
  });

  const handlers: Record<string, (a: Args) => unknown> = {
    get_settings: () => dto(),
    save_settings: (a) => {
      const next = a.settings as Settings;
      const dir = next.recapExportDir.trim();
      if (dir && !/^[A-Za-z]:[\\/]|^[\\/]{2}/.test(dir)) throw "The recap folder must be a full path, such as C:\\Users\\you\\Documents.";
      Object.assign(s, structuredClone(next));
      if (!s.model.trim()) s.model = "claude-haiku-5-5";
      // Dayflow drops a fallback equal to the main provider (config.rs sanitized).
      if (s.fallbackProvider === s.provider && s.fallbackBaseUrl.replace(/\/+$/, "") === s.baseUrl.replace(/\/+$/, "") && s.fallbackModel.trim() === s.model.trim()) {
        s.fallbackProvider = null;
      }
      const key = typeof a.apiKey === "string" ? a.apiKey.trim() : "";
      if (key) {
        const k = slot(s.provider, s.baseUrl);
        if (!k) throw "This provider does not use an API key";
        keys.add(k);
      }
      return dto();
    },
    test_provider: (a) => {
      if (v.has("test-fail"))
        throw "The API key was refused (401). It may be revoked or mistyped. Create a new key in the Claude Console under API keys, paste it above and select Save key.";
      const kind = a.provider as ProviderKind;
      const local = kind === "ollama" || kind === "lm_studio";
      return local ? "Blue (2140 ms)" : "Blue. (812 ms)";
    },
    list_models: (a) => {
      if (v.has("models-fail")) throw "error sending request for url (http://localhost:11434/api/tags): connection refused";
      const kind = a.provider as ProviderKind;
      if (kind === "ollama") return ["qwen3-vl:4b", "qwen3-vl:8b", "gemma4:12b"];
      if (kind === "claude_cli") return ["haiku", "sonnet", "opus"];
      return ["claude-haiku-5-5", "claude-sonnet-5", "claude-opus-5-5"];
    },
    provider_key_status: (a) => fallbackKeys.has(`${a.provider}:${a.baseUrl}`),
    provider_save_key: (a) => {
      fallbackKeys.add(`${a.provider}:${a.baseUrl}`);
      return true;
    },
    platform_info: () => ({
      version: "0.2.1",
      pauseOptions: [],
      minuteOptions: [5, 10, 15, 20, 30, 45, 60, 90, 120, 180, 240, 480],
      update: update(ctx.now() - 3600),
      damagedSettingsCopy: v.has("damaged") ? "C:\\Users\\Preview\\AppData\\Roaming\\dev.daytrace.app\\settings.json.bad" : null,
    }),
    platform_check_updates: () => update(ctx.now()),
    storage_health: () => health(),
  };
  const usage = base.storage_usage;
  const agent = base.storage_agent_setup;
  handlers.storage_usage = (a) => {
    if (v.has("storage-error")) throw "Access is denied. (os error 5) while reading C:\\Users\\Preview\\AppData\\Roaming\\dev.daytrace.app\\screenshots";
    return { ...(usage(a) as object), health: health() };
  };
  handlers.storage_agent_setup = (a) => ({ ...(agent(a) as object), installed: v.has("mcp-ok") });
  Object.assign(base, handlers);
}
