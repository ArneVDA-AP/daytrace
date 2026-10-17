import * as React from "react";
import { AppWindow, Plus, RefreshCw, X } from "lucide-react";
import { formatDuration } from "@/api";
import { useAppState } from "@/app/state";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import { toast } from "@/components/ui/toast";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { errorText } from "@/lib/utils";
import { platformApi, type AppCandidate } from "@/views/platform-api";
import { FieldError, Group, Note } from "./fields";
import { useSave } from "./save";

/** "open now", "seen 2 h ago", "seen yesterday". */
export function seenText(c: AppCandidate): string {
  if (c.running) return "Open now";
  if (c.lastSeen === null) return "";
  const ago = Date.now() / 1000 - c.lastSeen;
  if (ago < 3600) return "Seen in the last hour";
  if (ago < 86400) return `Seen ${formatDuration(ago).replace(/ \d+ min$/, "")} ago`;
  const d = Math.round(ago / 86400);
  return d === 1 ? "Seen yesterday" : `Seen ${d} days ago`;
}

/** Adds program names (one per line) to the private list, skipping ones already there. */
export function addPrivateApps(list: string[], text: string): string[] {
  const out = list.map((l) => l.trim()).filter(Boolean);
  for (const name of text.split(/[\r\n]+/).map((l) => l.trim())) {
    if (name && !out.some((l) => l.toLowerCase() === name.toLowerCase())) out.push(name);
  }
  return out;
}

/** Programs with an open window and programs from recent screenshots (platform_app_candidates). */
export function useAppCandidates(auto = false) {
  const [apps, setApps] = React.useState<AppCandidate[] | null>(null);
  const [loading, setLoading] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const load = React.useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      setApps(await platformApi.appCandidates());
    } catch (e) {
      setError(errorText(e));
    } finally {
      setLoading(false);
    }
  }, []);
  React.useEffect(() => {
    if (auto) void load();
  }, [auto, load]);
  return { apps, loading, error, load };
}

export const PRIVATE_APPS_HELP =
  "When one of these apps is in front, Daytrace takes no screenshot; when it is visible behind other windows, it is blanked. A name matches every program whose name contains it.";

/** Settings > Private apps: the list with remove, add by name, and a pick list of open and recent apps. */
export function PrivateAppsSection() {
  const app = useAppState();
  const list = app.settings.settings.blockedApps;
  const save = useSave();
  const id = React.useId();
  const [draft, setDraft] = React.useState("");
  const [error, setError] = React.useState<string | null>(null);
  const picker = useAppCandidates();
  const listRef = React.useRef<HTMLUListElement>(null);

  // Typed into "Add" but not added when leaving: add it then.
  const draftRef = React.useRef(draft);
  draftRef.current = draft;
  React.useEffect(
    () => () => {
      const d = draftRef.current.trim();
      if (d) void save((cur) => ({ blockedApps: addPrivateApps(cur.blockedApps, d) })).catch(() => undefined);
    },
    [save],
  );

  async function add(text: string) {
    setError(null);
    try {
      await save((cur) => ({ blockedApps: addPrivateApps(cur.blockedApps, text) }));
      return true;
    } catch (e) {
      setError(`Couldn’t add it: ${errorText(e)}`);
      return false;
    }
  }

  async function remove(name: string, index: number) {
    setError(null);
    try {
      await save((cur) => ({ blockedApps: cur.blockedApps.filter((a) => a !== name) }));
      // Keep keyboard focus in the list: the next row's button, or the add field.
      requestAnimationFrame(() => {
        const buttons = listRef.current?.querySelectorAll<HTMLButtonElement>("button[data-remove]");
        const next = buttons?.[Math.min(index, (buttons?.length ?? 1) - 1)];
        (next ?? document.getElementById(`${id}-add`))?.focus();
      });
      toast(`${name} can be captured again`, {
        description: "Removed from private apps.",
        action: { label: "Undo", onClick: () => void add(name) },
      });
    } catch (e) {
      setError(`Couldn’t remove it: ${errorText(e)}`);
    }
  }

  const listed = new Set(list.map((b) => b.trim().toLowerCase()));
  const shown = (picker.apps ?? []).filter((a) => !listed.has(a.name.toLowerCase()));

  return (
    <>
      <Group title="Never captured" description={PRIVATE_APPS_HELP}>
        {list.length === 0 ? (
          <p className="px-4 py-3 text-meta text-muted-foreground">No private apps yet. Add password managers, banking or messaging apps you never want in a screenshot.</p>
        ) : (
          <ul ref={listRef} aria-label="Private apps" className="flex flex-col divide-y">
            {list.map((a, i) => (
              <li key={a} className="flex items-center gap-3 px-4 py-2">
                <span className="grid size-7 shrink-0 place-items-center rounded-md bg-surface-2 text-muted-foreground">
                  <AppWindow className="size-4" aria-hidden />
                </span>
                <span className="min-w-0 flex-1 truncate text-body" translate="no">
                  {a}
                </span>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button data-remove variant="ghost" size="icon-sm" aria-label={`Remove ${a}`} onClick={() => void remove(a, i)}>
                      <X aria-hidden />
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent>Remove</TooltipContent>
                </Tooltip>
              </li>
            ))}
          </ul>
        )}
        <form
          className="flex flex-col gap-2 px-4 py-3"
          onSubmit={async (e) => {
            e.preventDefault();
            if (draft.trim() && (await add(draft))) setDraft("");
          }}
        >
          <label htmlFor={`${id}-add`} className="text-meta font-medium">
            Add an app by name
          </label>
          <div className="flex flex-wrap gap-2">
            <Input
              id={`${id}-add`}
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              placeholder="Program name, such as Signal"
              autoComplete="off"
              spellCheck={false}
              className="min-w-0 flex-1 basis-56"
            />
            <Button type="submit" variant="secondary" disabled={!draft.trim()}>
              <Plus aria-hidden /> Add
            </Button>
          </div>
          <FieldError>{error}</FieldError>
        </form>
      </Group>

      <Group
        title="Open and recent apps"
        description="Programs with an open window now, and programs Daytrace has seen in front recently."
        actions={
          <Button variant="outline" size="sm" onClick={() => void picker.load()} disabled={picker.loading}>
            {picker.loading ? <Spinner aria-hidden role={undefined} aria-label={undefined} /> : picker.apps ? <RefreshCw aria-hidden /> : null}
            {picker.loading ? "Looking for apps…" : picker.apps ? "Refresh the list" : "Show open and recent apps"}
          </Button>
        }
      >
        <div aria-live="polite" className="contents">
          {!picker.apps && !picker.error && <p className="px-4 py-3 text-meta text-muted-foreground">Select Show open and recent apps to pick from them.</p>}
          {picker.error && (
            <div className="px-4 py-3">
              <FieldError>{`Couldn’t list apps: ${picker.error}`}</FieldError>
            </div>
          )}
          {picker.apps && shown.length === 0 && !picker.error && (
            <p className="px-4 py-3 text-meta text-muted-foreground">No other apps found. Apps show up here once Daytrace has seen them in front.</p>
          )}
          {shown.length > 0 && (
            <ul aria-label="Apps you can add" className="flex flex-col divide-y">
              {shown.map((c) => (
                <li key={c.name} className="flex items-center gap-3 px-4 py-2">
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-body" translate="no">
                      {c.name}
                    </span>
                    <span className="block text-caption text-muted-foreground">{seenText(c)}</span>
                  </span>
                  <Button variant="outline" size="sm" aria-label={`Add ${c.name} to private apps`} onClick={() => void add(c.name)}>
                    <Plus aria-hidden /> Add
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </div>
      </Group>
      <Note>Changes apply to the next screenshot. Screenshots already taken stay until they are deleted with the retention period or under Storage.</Note>
    </>
  );
}
