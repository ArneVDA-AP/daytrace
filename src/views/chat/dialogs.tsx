import * as React from "react";
import { Cpu, ExternalLink } from "lucide-react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogFooter, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Skeleton } from "@/components/ui/skeleton";
import { toast } from "@/components/ui/toast";
import { navigate } from "@/lib/router";
import { errorText } from "@/lib/utils";
import { chatApi, type ChatInfo, type ChatMemory, type Conversation } from "@/views/chat-api";
import { dateTimeOf } from "@/views/chat/lib";

/** The model chip in the header: which model answers, and the chat-only model override. */
export function ModelPicker({ info, onSaved }: { info: ChatInfo; onSaved: (i: ChatInfo) => void }) {
  const [open, setOpen] = React.useState(false);
  const [draft, setDraft] = React.useState(info.chatModel);
  const [saving, setSaving] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    setSaving(true);
    setError(null);
    try {
      const next = await chatApi.setModel(draft.trim());
      onSaved(next);
      setOpen(false);
      toast(next.chatModel ? `Chat uses ${next.chatModel}` : "Chat uses the timeline model");
    } catch (err) {
      setError(errorText(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <Popover
      open={open}
      onOpenChange={(o) => {
        setOpen(o);
        if (o) {
          setDraft(info.chatModel);
          setError(null);
        }
      }}
    >
      <PopoverTrigger asChild>
        <Button variant="outline" size="sm" className="max-w-64 min-w-0" aria-label={`Chat model: ${info.model}, ${info.providerLabel}. Change`}>
          <Cpu aria-hidden="true" />
          <span className="truncate" translate="no">
            {info.model || "No model"}
          </span>
          <span className="hidden truncate font-normal text-subtle-foreground wide:inline" translate="no">
            · {info.providerLabel}
          </span>
        </Button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-80">
        <form onSubmit={save} className="flex flex-col gap-3">
          <div className="flex flex-col gap-1">
            <p className="text-body font-semibold">Chat model</p>
            <p className="text-meta text-muted-foreground">
              Answers come from <span translate="no">{info.providerLabel}</span>. The provider is set in Settings.
            </p>
          </div>
          <div className="flex flex-col gap-1.5">
            <label htmlFor="chat-model" className="text-meta font-medium">
              Model for chat only
            </label>
            <Input
              id="chat-model"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              placeholder={info.timelineModel || "Model name…"}
              spellCheck={false}
              autoComplete="off"
              translate="no"
              aria-describedby="chat-model-hint"
            />
            <p id="chat-model-hint" className="text-caption text-muted-foreground">
              Leave empty to use the timeline model
              {info.timelineModel ? (
                <>
                  ,{" "}
                  <span translate="no" className="whitespace-nowrap">
                    {info.timelineModel}
                  </span>
                </>
              ) : (
                ""
              )}
              .
            </p>
            {error && (
              <p role="alert" className="text-caption text-destructive">
                {error}
              </p>
            )}
          </div>
          <div className="flex items-center justify-between gap-2">
            <Button type="button" variant="ghost" size="sm" onClick={() => navigate("settings", { section: "provider" })}>
              Provider settings
            </Button>
            <Button type="submit" size="sm" disabled={saving || draft.trim() === info.chatModel}>
              {saving ? "Saving…" : "Save"}
            </Button>
          </div>
        </form>
      </PopoverContent>
    </Popover>
  );
}

/** Dayflow's chat memory: notes on the person's preferences, updated from answers, editable here. */
export function MemoryDialog({ open, onOpenChange }: { open: boolean; onOpenChange: (o: boolean) => void }) {
  const [memory, setMemory] = React.useState<ChatMemory | null>(null);
  const [draft, setDraft] = React.useState("");
  const [error, setError] = React.useState<string | null>(null);
  const [saving, setSaving] = React.useState(false);

  React.useEffect(() => {
    if (!open) return;
    let live = true;
    setError(null);
    chatApi
      .getMemory()
      .then((m) => {
        if (!live) return;
        setMemory(m);
        setDraft(m.content);
      })
      .catch((e) => live && setError(errorText(e)));
    return () => {
      live = false;
    };
  }, [open]);

  const save = async (content: string, done: string, undo?: string) => {
    setSaving(true);
    setError(null);
    try {
      const m = await chatApi.saveMemory(content);
      setMemory(m);
      setDraft(m.content);
      onOpenChange(false);
      toast(done, undo !== undefined ? { action: { label: "Undo", onClick: () => void chatApi.saveMemory(undo).catch(() => {}) } } : {});
    } catch (e) {
      setError(errorText(e));
    } finally {
      setSaving(false);
    }
  };

  const dirty = memory !== null && draft !== memory.content;
  const max = memory?.maxChars ?? 10000;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-xl">
        <DialogHeader>
          <DialogTitle className="text-title font-display">Chat memory</DialogTitle>
          <DialogDescription className="text-meta">
            Chat keeps short notes about you and how you like answers, and updates them from your conversations. They are sent with every question. Edit or
            clear them here.
          </DialogDescription>
        </DialogHeader>
        {memory === null && !error ? (
          <Skeleton className="h-40 w-full" />
        ) : (
          <div className="flex flex-col gap-1.5">
            <label htmlFor="chat-memory" className="sr-only">
              Memory text
            </label>
            <textarea
              id="chat-memory"
              rows={8}
              value={draft}
              maxLength={max}
              onChange={(e) => setDraft(e.target.value)}
              placeholder={"Profile: …\nStyle: …"}
              disabled={memory === null}
              className="scroll-thin min-h-40 w-full resize-y rounded-md border border-input bg-surface-1 px-3 py-2 text-meta leading-5 shadow-xs placeholder:text-muted-foreground disabled:opacity-50"
            />
            <p className="flex flex-wrap justify-between gap-x-4 text-caption text-muted-foreground tnum">
              <span>
                {draft.length.toLocaleString("en-US")} of {max.toLocaleString("en-US")} characters
              </span>
              <span>{memory?.updatedAt ? `Last updated ${dateTimeOf(memory.updatedAt)}` : "Not saved yet"}</span>
            </p>
          </div>
        )}
        {error && (
          <p role="alert" className="text-meta text-destructive">
            {error}
          </p>
        )}
        <DialogFooter className="sm:justify-between">
          <Button
            variant="ghost"
            className="text-destructive hover:text-destructive"
            disabled={saving || !memory?.content}
            onClick={() => memory && save("", "Memory cleared", memory.content)}
          >
            Clear memory
          </Button>
          <div className="flex flex-col-reverse gap-2 sm:flex-row">
            <Button variant="outline" onClick={() => onOpenChange(false)}>
              Cancel
            </Button>
            <Button disabled={saving || !dirty} onClick={() => save(draft, "Memory saved")}>
              {saving ? "Saving…" : "Save"}
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

/**
 * The question before a link from an answer opens. Answers are shaped by text read from the
 * screen, so a link may point somewhere other than its words say (or carry data in its address):
 * the host and the full address are shown.
 */
export function LinkDialog({
  target,
  onClose,
  onOpen,
}: {
  target: { url: string; host: string; address: string } | null;
  onClose: () => void;
  onOpen: (url: string) => void;
}) {
  return (
    <AlertDialog open={target !== null} onOpenChange={(o) => !o && onClose()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle className="font-display text-title">Open this link in your browser?</AlertDialogTitle>
          <AlertDialogDescription className="text-meta">
            Links in answers can come from text on your screen. Open it only if you know this website.
          </AlertDialogDescription>
        </AlertDialogHeader>
        {target && (
          <dl className="grid gap-x-4 gap-y-2 rounded-md border bg-surface-2 p-3 text-meta sm:grid-cols-[auto_minmax(0,1fr)]">
            <dt className="text-muted-foreground">Website</dt>
            <dd className="font-semibold [overflow-wrap:anywhere]" translate="no">
              {target.host}
            </dd>
            <dt className="text-muted-foreground">Full address</dt>
            <dd className="font-mono text-caption [overflow-wrap:anywhere]" translate="no">
              {target.address}
            </dd>
          </dl>
        )}
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction onClick={() => target && onOpen(target.url)}>
            <ExternalLink aria-hidden="true" /> Open in browser
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}

export function DeleteDialog({
  conversation,
  onClose,
  onDelete,
}: {
  conversation: Conversation | null;
  onClose: () => void;
  onDelete: (c: Conversation) => void;
}) {
  return (
    <AlertDialog open={conversation !== null} onOpenChange={(o) => !o && onClose()}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle className="font-display text-title">Delete this chat?</AlertDialogTitle>
          <AlertDialogDescription className="text-meta">
            {conversation && (
              <>
                “{conversation.title}” and its {conversation.messageCount} {conversation.messageCount === 1 ? "message" : "messages"} are removed from this PC.
                This can’t be undone.
              </>
            )}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction variant="destructive" onClick={() => conversation && onDelete(conversation)}>
            Delete chat
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
