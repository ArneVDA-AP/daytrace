import * as React from "react";
import { RotateCcw } from "lucide-react";
import { api, type PromptOverrides } from "@/api";
import { useAppState } from "@/app/state";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Textarea } from "@/components/ui/textarea";
import { useDelayedLoading } from "@/lib/use-delayed";
import { errorText } from "@/lib/utils";
import { ErrorBanner, FieldError, Group, Note, Row } from "./fields";
import { useCommitField, useSave } from "./save";

const BLOCKS: { key: keyof PromptOverrides; label: string; help: string }[] = [
  { key: "detailedTitle", label: "Detailed mode: titles", help: "How the model writes card titles." },
  { key: "detailedSummary", label: "Detailed mode: summaries", help: "How the model writes card summaries." },
  { key: "lightweightTitle", label: "Lightweight mode: titles", help: "The title instructions for small local models." },
  { key: "lightweightSummary", label: "Lightweight mode: summaries", help: "The summary guidelines and examples for small local models." },
];

const NO_OVERRIDES: PromptOverrides = { detailedTitle: "", detailedSummary: "", lightweightTitle: "", lightweightSummary: "" };

/** Settings > Language and prompts: the output language and the editable card instructions. */
export function PromptsSection() {
  const app = useAppState();
  const s = app.settings.settings;
  const save = useSave();
  const id = React.useId();
  const language = useCommitField(s.outputLanguage, (v) => save({ outputLanguage: v.trim() }));

  const [defaults, setDefaults] = React.useState<PromptOverrides | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [resetError, setResetError] = React.useState<string | null>(null);
  const load = React.useCallback(() => {
    setError(null);
    api
      .providerPromptDefaults()
      .then(setDefaults)
      .catch((e) => setError(errorText(e)));
  }, []);
  React.useEffect(load, [load]);
  const showSkeleton = useDelayedLoading(!defaults && !error);

  const po = s.promptOverrides;
  const edited = BLOCKS.filter((b) => po[b.key].trim() !== "").length;

  return (
    <>
      <Group title="Language">
        <Row
          label="Write cards, recaps and chat answers in"
          htmlFor={`${id}-lang`}
          description="Leave it empty for English. The app itself stays in English."
        >
          <Input id={`${id}-lang`} {...language.bind} placeholder="English" autoComplete="off" className="w-56" />
          <FieldError>{language.error}</FieldError>
        </Row>
      </Group>

      <Group
        title="Prompt instructions"
        description="Change how the model writes card titles and summaries. These instructions are part of the Detailed and Lightweight prompts, also when they run on the fallback provider. A block is saved when you leave it; Ctrl+Enter saves it now, Esc drops your typing."
        actions={
          edited > 0 && defaults ? (
            <Button
              variant="ghost"
              size="sm"
              onClick={() => {
                setResetError(null);
                save({ promptOverrides: NO_OVERRIDES }).catch((e) => setResetError(`Couldn’t reset the instructions: ${errorText(e)}`));
              }}
            >
              <RotateCcw aria-hidden /> Reset all to default
            </Button>
          ) : undefined
        }
      >
        {error && (
          <div className="p-3">
            <ErrorBanner
              title="Couldn’t load the default instructions"
              message={error}
              action={
                <Button size="sm" variant="outline" onClick={load}>
                  Try again
                </Button>
              }
            />
          </div>
        )}
        {!defaults && !error && (
          <div className="flex flex-col gap-3 px-4 py-4" aria-busy="true">
            {showSkeleton && (
              <>
                <Skeleton className="h-4 w-48" />
                <Skeleton className="h-28 w-full" />
                <Skeleton className="h-4 w-56" />
                <Skeleton className="h-28 w-full" />
              </>
            )}
          </div>
        )}
        {resetError && (
          <div className="px-4 py-3">
            <FieldError>{resetError}</FieldError>
          </div>
        )}
        {defaults && BLOCKS.map((b) => <PromptBlock key={b.key} block={b} defaults={defaults} />)}
      </Group>
      <Note>
        The one-step prompt of the Claude Code and Codex CLI providers can’t be edited. Daytrace writes each card’s step-by-step log from the screenshot
        transcription, so there is no instruction for it.
      </Note>
    </>
  );
}

function PromptBlock({ block, defaults }: { block: (typeof BLOCKS)[number]; defaults: PromptOverrides }) {
  const app = useAppState();
  const save = useSave();
  const id = React.useId();
  const stored = app.settings.settings.promptOverrides[block.key];
  const [resetError, setResetError] = React.useState<string | null>(null);
  const custom = stored.trim() !== "";
  // Text equal to the default is stored as "no override", so later default updates still apply.
  const field = useCommitField(custom ? stored : defaults[block.key], (v) =>
    save((cur) => ({ promptOverrides: { ...cur.promptOverrides, [block.key]: v.trim() === defaults[block.key].trim() ? "" : v } })),
    { multiline: true },
  );

  return (
    <Row
      stack
      htmlFor={id}
      label={
        <span className="flex flex-wrap items-center gap-2">
          {block.label}
          <span className="rounded-sm bg-surface-2 px-1.5 py-0.5 text-caption font-normal text-muted-foreground">{custom || field.dirty ? "Edited" : "Default"}</span>
        </span>
      }
      description={block.help}
    >
      <Textarea id={id} {...field.bind} spellCheck={false} className="max-h-80 font-mono text-meta" />
      {custom && (
        <div className="flex justify-end">
          <Button
            variant="ghost"
            size="sm"
            onClick={() => {
              field.revert();
              setResetError(null);
              save((cur) => ({ promptOverrides: { ...cur.promptOverrides, [block.key]: "" } })).catch((e) =>
                setResetError(`Couldn’t reset to the default: ${errorText(e)}`),
              );
            }}
          >
            <RotateCcw aria-hidden /> Reset to default
          </Button>
        </div>
      )}
      <FieldError>{field.error ?? resetError}</FieldError>
    </Row>
  );
}
