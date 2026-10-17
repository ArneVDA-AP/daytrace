import * as React from "react";
import { History, Info, NotebookPen, RotateCcw, SquarePen } from "lucide-react";
import { useAppState } from "@/app/state";
import { useViewProps } from "@/app/view-props";
import { Banner } from "@/components/banner";
import { PageHeader } from "@/components/page-header";
import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { toast } from "@/components/ui/toast";
import { numParam, setParams, useRoute } from "@/lib/router";
import { useDelayedLoading } from "@/lib/use-delayed";
import { errorText } from "@/lib/utils";
import { chatApi, linkTarget, type ChatInfo, type ChatMessage, type ChatProgress, type Conversation } from "@/views/chat-api";
import { Composer } from "@/views/chat/composer";
import { DeleteDialog, LinkDialog, MemoryDialog, ModelPicker } from "@/views/chat/dialogs";
import { ConversationList } from "@/views/chat/history";
import { plainAnswer } from "@/views/chat/lib";
import { AssistantMessage, ErrorMessage, StepList, UserMessage, type Step } from "@/views/chat/message";

/*
 * Chat: questions about the timeline, answered by the chat model with tools that read the cards
 * (Dayflow's dashboard chat). History of saved chats on the left (a popover below 960 px), the
 * open chat in the URL (#/chat?c=12), answers with a card chip after each sentence that cites a
 * card, tool steps while an answer is written, Stop (Esc), Try again, model and memory.
 */

/** The open chat survives leaving the section (the rail links to #/chat without ?c=). */
let lastConversationId: number | null = null;
/** So does a question typed but not sent (Ctrl+2 and back keeps it). */
let draftInput = "";

export default function ChatSection() {
  const { today, colorFor, settings } = useViewProps();
  const { analysis, problems } = useAppState();
  const route = useRoute();
  const cParam = numParam(route.params, "c");
  const activeId = route.section === "chat" ? cParam : null;

  const [info, setInfo] = React.useState<ChatInfo | null>(null);
  const [infoError, setInfoError] = React.useState<string | null>(null);
  const [conversations, setConversations] = React.useState<Conversation[] | null>(null);
  const [listError, setListError] = React.useState<string | null>(null);
  const [messages, setMessages] = React.useState<ChatMessage[]>([]);
  const [loadingConv, setLoadingConv] = React.useState(false);
  const [convError, setConvError] = React.useState<string | null>(null);
  const [pending, setPending] = React.useState<string | null>(null);
  const [busy, setBusy] = React.useState(false);
  const [stopping, setStopping] = React.useState(false);
  const [steps, setSteps] = React.useState<Step[]>([]);
  const [stage, setStage] = React.useState<"thinking" | "tools">("thinking");
  /** A question that could not be sent, and whether it went back into the empty field. */
  const [sendError, setSendError] = React.useState<{ message: string; question: string; restored: boolean } | null>(null);
  const [input, setInputState] = React.useState(() => draftInput);
  const inputNow = React.useRef(input);
  const setInput = React.useCallback((v: string) => {
    draftInput = v;
    inputNow.current = v;
    setInputState(v);
  }, []);
  const [copied, setCopied] = React.useState<number | null>(null);
  const [toDelete, setToDelete] = React.useState<Conversation | null>(null);
  const [link, setLink] = React.useState<ReturnType<typeof linkTarget>>(null);
  const [memoryOpen, setMemoryOpen] = React.useState(false);
  const [historyOpen, setHistoryOpen] = React.useState(false);
  const [announce, setAnnounce] = React.useState("");
  /** Bumped to read the open chat again (an answer from elsewhere finished). */
  const [reloadTick, setReloadTick] = React.useState(0);

  const scrollRef = React.useRef<HTMLDivElement>(null);
  const inputRef = React.useRef<HTMLTextAreaElement>(null);
  const busyRef = React.useRef(false);
  const loadedRef = React.useRef<number | null>(null);
  /** The chat whose messages are on screen (kept while the same chat is read again). */
  const shownRef = React.useRef<number | null>(null);
  const nearBottom = React.useRef(true);
  const scrollReq = React.useRef<"bottom" | "lastQuestion" | null>(null);

  const openConversation = React.useCallback((id: number | null) => {
    lastConversationId = id;
    setConvError(null);
    setSendError(null);
    setHistoryOpen(false);
    setParams({ c: id }, { section: "chat" });
  }, []);

  // Back to the chat that was open when the section was left; a new chat starts in the field.
  React.useEffect(() => {
    if (cParam === null && lastConversationId !== null) setParams({ c: lastConversationId }, { section: "chat" });
    else if (cParam === null) requestAnimationFrame(() => inputRef.current?.focus());
    // Only on entering the section.
  }, []);

  const loadList = React.useCallback(
    () =>
      chatApi
        .list()
        .then((l) => {
          setConversations(l);
          setListError(null);
        })
        .catch((e) => setListError(errorText(e))),
    [],
  );
  const loadInfo = React.useCallback(
    () =>
      chatApi
        .info()
        .then((i) => {
          setInfo(i);
          setInfoError(null);
        })
        .catch((e) => setInfoError(errorText(e))),
    [],
  );

  React.useEffect(() => {
    void loadList();
  }, [loadList]);

  // Provider settings may change in Settings (or the model from here).
  React.useEffect(() => {
    void loadInfo();
  }, [settings, loadInfo]);

  // The open chat follows the URL.
  React.useEffect(() => {
    if (activeId !== null) lastConversationId = activeId;
    if (activeId === null) {
      loadedRef.current = null;
      shownRef.current = null;
      setMessages([]);
      return;
    }
    if (loadedRef.current === activeId) return;
    let live = true;
    setLoadingConv(true);
    if (shownRef.current !== activeId) setMessages([]);
    chatApi
      .get(activeId)
      .then((d) => {
        if (!live) return;
        loadedRef.current = activeId;
        shownRef.current = activeId;
        // Open at the latest question, so its answer reads from the top.
        scrollReq.current = "lastQuestion";
        setMessages(d.messages);
      })
      .catch((e) => {
        if (!live) return;
        lastConversationId = null;
        loadedRef.current = null;
        shownRef.current = null;
        setMessages([]);
        setConvError(errorText(e));
        setParams({ c: null }, { section: "chat" });
      })
      .finally(() => live && setLoadingConv(false));
    return () => {
      live = false;
    };
  }, [activeId, reloadTick]);

  // A question still being answered when the section was left (or from another window): wait for
  // it, then show the chat that was updated last.
  React.useEffect(() => {
    if (!info?.busy || busy) return;
    const t = setInterval(async () => {
      const i = await chatApi.info().catch(() => null);
      if (i && !i.busy) {
        clearInterval(t);
        setInfo(i);
        setStopping(false);
        const list = await chatApi.list().catch(() => null);
        if (list) {
          setConversations(list);
          if (list.length) {
            loadedRef.current = null;
            openConversation(list[0].id);
            setReloadTick((n) => n + 1);
          }
        }
      }
    }, 1500);
    return () => clearInterval(t);
  }, [info?.busy, busy, openConversation]);

  // One listener for the section's lifetime; events count only while this window asked.
  React.useEffect(() => {
    const un = chatApi.onProgress((p: ChatProgress) => {
      if (!busyRef.current) return;
      if (p.stage === "thinking") {
        setStage("thinking");
      } else if (p.stage === "toolStarted") {
        setStage("tools");
        setSteps((s) => [...s, { label: p.label }]);
      } else {
        setSteps((s) => {
          const copy = [...s];
          const idx = copy.map((x) => x.summary === undefined).lastIndexOf(true);
          if (idx >= 0) copy[idx] = { ...copy[idx], summary: p.summary, ok: p.ok };
          return copy;
        });
      }
    });
    return () => {
      void un.then((f) => f());
    };
  }, []);

  React.useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const req = scrollReq.current;
    scrollReq.current = null;
    if (req === "lastQuestion") {
      const users = el.querySelectorAll<HTMLElement>('[data-role="user"]');
      const last = users[users.length - 1];
      if (last) {
        el.scrollTop += last.getBoundingClientRect().top - el.getBoundingClientRect().top - 16;
        return;
      }
    }
    if (req === "bottom" || nearBottom.current) el.scrollTop = el.scrollHeight;
  }, [messages, pending, steps, busy]);

  const blocked = info?.problem ?? null;
  const othersBusy = !busy && !!info?.busy;

  /**
   * Sends a question. `fromField`: it is the text in the field, which empties; a suggestion, a
   * welcome prompt or Try again leave what is typed there alone.
   */
  const send = async (raw: string, fromField: boolean) => {
    const text = raw.trim();
    if (!text || busy || info?.busy || blocked) return;
    setSendError(null);
    setConvError(null);
    setPending(text);
    if (fromField) setInput("");
    setSteps([]);
    setStage("thinking");
    setBusy(true);
    setAnnounce("");
    busyRef.current = true;
    scrollReq.current = "bottom";
    try {
      const d = await chatApi.send(activeId, text);
      lastConversationId = d.conversation.id;
      loadedRef.current = d.conversation.id;
      shownRef.current = d.conversation.id;
      scrollReq.current = "lastQuestion";
      setMessages(d.messages);
      setParams({ c: d.conversation.id }, { section: "chat" });
      void loadList();
      const last = d.messages[d.messages.length - 1];
      setAnnounce(
        last?.role === "assistant"
          ? `Answered.${last.citations.length ? ` ${last.citations.length} ${last.citations.length === 1 ? "card" : "cards"} cited.` : ""}`
          : `No answer. ${last?.content ?? ""}`,
      );
    } catch (e) {
      // Nothing was stored (for example the provider is not set up). The question goes back into
      // the field when it is empty; newer typing there is kept.
      const restored = !inputNow.current.trim();
      if (restored) setInput(text);
      setSendError({ message: errorText(e), question: text, restored });
    } finally {
      busyRef.current = false;
      setPending(null);
      setBusy(false);
      setStopping(false);
      void loadInfo();
      inputRef.current?.focus();
    }
  };

  const stop = () => {
    setStopping(true);
    chatApi.cancel().catch((e) => {
      setStopping(false);
      toast("Couldn’t stop the answer", { description: errorText(e), tone: "error" });
    });
  };

  const retry = () => {
    const lastUser = [...messages].reverse().find((m) => m.role === "user");
    if (lastUser) void send(lastUser.content, false);
  };

  const remove = async (c: Conversation) => {
    setToDelete(null);
    if (busy || c.id === info?.busyConversationId) return;
    try {
      await chatApi.remove(c.id);
      if (c.id === activeId) openConversation(null);
      if (c.id === lastConversationId) lastConversationId = null;
      void loadList();
      toast("Chat deleted");
    } catch (e) {
      toast("Couldn’t delete the chat", { description: errorText(e), tone: "error" });
    }
  };

  const copy = async (m: ChatMessage) => {
    try {
      await navigator.clipboard.writeText(plainAnswer(m.content));
      setCopied(m.id);
      setTimeout(() => setCopied((c) => (c === m.id ? null : c)), 1500);
      toast("Answer copied");
    } catch {
      toast("Couldn’t copy to the clipboard", { tone: "error" });
    }
  };

  const onLink = React.useCallback((url: string) => setLink(linkTarget(url)), []);

  const active = conversations?.find((c) => c.id === activeId) ?? null;
  const empty = messages.length === 0 && pending === null && !loadingConv;
  const showConvSkeleton = useDelayedLoading(loadingConv);
  const showListSkeleton = useDelayedLoading(conversations === null && !listError);
  const lastMsg = messages[messages.length - 1];
  const suggestions = !busy && lastMsg?.role === "assistant" ? lastMsg.suggestions : [];
  const analysing = analysis.tone === "busy";

  const list = (prefix: string) => (
    <>
      {listError && conversations === null ? (
        <div className="flex flex-col items-start gap-2 px-3 py-2">
          <p className="text-meta text-muted-foreground">Couldn’t load your chats. {listError}</p>
          <Button size="sm" variant="outline" onClick={() => void loadList()}>
            <RotateCcw aria-hidden="true" /> Try again
          </Button>
        </div>
      ) : (
        <ConversationList
          conversations={conversations}
          loading={showListSkeleton}
          activeId={activeId}
          locked={busy}
          busyConversationId={info?.busyConversationId ?? null}
          onOpen={(id) => {
            if (id !== activeId) openConversation(id);
            else setHistoryOpen(false);
          }}
          onDelete={setToDelete}
          idPrefix={prefix}
        />
      )}
    </>
  );

  return (
    <>
      <PageHeader
        label="Chat"
        title={active?.title ?? (activeId !== null ? "Chat" : "New chat")}
        actions={
          <>
            <Popover open={historyOpen} onOpenChange={setHistoryOpen}>
              <Tooltip>
                <TooltipTrigger asChild>
                  <PopoverTrigger asChild>
                    <Button variant="outline" size="icon-sm" className="split:hidden" aria-label="History">
                      <History aria-hidden="true" />
                    </Button>
                  </PopoverTrigger>
                </TooltipTrigger>
                <TooltipContent>History</TooltipContent>
              </Tooltip>
              <PopoverContent align="end" className="scroll-thin max-h-[min(32rem,70vh)] w-80 overflow-y-auto p-2">
                <h2 className="px-3 pt-1 pb-2 text-body font-semibold">History</h2>
                {list("pop")}
              </PopoverContent>
            </Popover>
            {info && <ModelPicker info={info} onSaved={setInfo} />}
            <Tooltip>
              <TooltipTrigger asChild>
                <Button variant="outline" size="sm" onClick={() => setMemoryOpen(true)} aria-label="Memory" className="max-wide:size-7 max-wide:px-0">
                  <NotebookPen aria-hidden="true" />
                  <span className="hidden wide:inline">Memory</span>
                </Button>
              </TooltipTrigger>
              <TooltipContent className="wide:hidden">Memory</TooltipContent>
            </Tooltip>
            <Button
              variant="outline"
              size="sm"
              onClick={() => {
                openConversation(null);
                requestAnimationFrame(() => inputRef.current?.focus());
              }}
              disabled={busy || (activeId === null && messages.length === 0)}
            >
              <SquarePen aria-hidden="true" /> New chat
            </Button>
          </>
        }
      />
      <div className="flex min-h-0 flex-1">
        <section aria-labelledby="chat-history-title" className="scroll-thin hidden w-64 shrink-0 flex-col gap-2 overflow-y-auto border-r px-2 py-3 split:flex">
          <h2 id="chat-history-title" className="px-3 text-body font-semibold">
            History
          </h2>
          {list("col")}
        </section>

        <div className="flex min-w-0 flex-1 flex-col">
          <div
            ref={scrollRef}
            onScroll={(e) => {
              const el = e.currentTarget;
              nearBottom.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
            }}
            className="scroll-thin min-h-0 flex-1 overflow-y-auto overscroll-contain"
          >
            <div className="mx-auto flex min-h-full w-full max-w-3xl flex-col gap-4 px-(--page-x) py-5">
              {infoError && (
                <Banner
                  tone="error"
                  title="Couldn’t read the chat settings"
                  actions={
                    <Button size="sm" variant="outline" onClick={() => void loadInfo()}>
                      Try again
                    </Button>
                  }
                >
                  {infoError}
                </Banner>
              )}
              {convError && (
                <Banner tone="error" title="Couldn’t open that chat">
                  {convError} A new chat is open instead.
                </Banner>
              )}

              {showConvSkeleton && messages.length === 0 && <ConversationSkeleton />}

              {empty && !showConvSkeleton && (
                <Welcome
                  prompts={info?.welcomePrompts ?? []}
                  providerLabel={info?.providerLabel ?? null}
                  disabled={!!blocked || busy || !!info?.busy}
                  onAsk={(q) => void send(q, false)}
                />
              )}

              {(messages.length > 0 || pending !== null) && (
                <ol className="flex flex-col gap-6" aria-label="Conversation">
                  {messages.map((m, i) => (
                    <li key={m.id}>
                      {m.role === "user" ? (
                        <UserMessage text={m.content} />
                      ) : m.role === "error" ? (
                        <ErrorMessage m={m} onRetry={i === messages.length - 1 ? retry : undefined} retryDisabled={busy || !!info?.busy || !!blocked} />
                      ) : (
                        <AssistantMessage m={m} today={today} colorFor={colorFor} onLink={onLink} onCopy={() => void copy(m)} copied={copied === m.id} />
                      )}
                    </li>
                  ))}
                  {pending !== null && (
                    <li>
                      <UserMessage text={pending} />
                    </li>
                  )}
                  {busy && (
                    <li className="flex flex-col gap-2">
                      <p className="flex items-center gap-2 text-meta text-muted-foreground" role="status">
                        {steps.length === 0 || stage === "thinking" ? "Thinking…" : "Reading your timeline…"}
                        {stopping && " Stopping…"}
                      </p>
                      {steps.length > 0 && <StepList steps={steps} running />}
                      {steps.length === 0 && <Skeleton className="h-4 w-2/3 max-w-80" />}
                    </li>
                  )}
                </ol>
              )}
              {suggestions.length > 0 && (
                <section aria-labelledby="chat-follow-up" className="mt-2 flex flex-col gap-2">
                  <h2 id="chat-follow-up" className="text-caption text-muted-foreground">
                    Follow up
                  </h2>
                  <ul className="flex flex-wrap gap-2">
                    {suggestions.map((s) => (
                      <li key={s}>
                        <Button
                          variant="outline"
                          size="sm"
                          className="h-auto min-h-7 rounded-full py-1 text-left whitespace-normal"
                          disabled={!!blocked || !!info?.busy}
                          onClick={() => void send(s, false)}
                        >
                          {s}
                        </Button>
                      </li>
                    ))}
                  </ul>
                </section>
              )}
            </div>
          </div>

          <div className="relative shrink-0 px-(--page-x) pt-2 pb-4">
            {/* Text scrolling under the composer fades out instead of being cut. */}
            <div aria-hidden="true" className="pointer-events-none absolute -top-6 right-3 left-0 h-6 bg-linear-to-t from-surface-1 to-transparent" />
            <div className="mx-auto flex w-full max-w-3xl flex-col gap-2">
              {othersBusy && (
                <p className="flex items-center gap-2 text-caption text-muted-foreground" role="status">
                  <Info className="size-3.5 shrink-0" aria-hidden="true" />
                  Still answering an earlier question. You can stop it, or wait and ask again.
                </p>
              )}
              {analysing && !othersBusy && (
                <p className="flex items-center gap-2 text-caption text-muted-foreground">
                  <Info className="size-3.5 shrink-0" aria-hidden="true" />
                  Daytrace is writing timeline cards. Answers use the cards that exist now.
                </p>
              )}
              {sendError && (
                <Banner
                  tone="error"
                  title="Couldn’t send the question"
                  actions={
                    <Button
                      size="sm"
                      variant="outline"
                      onClick={() => void send(sendError.question, input.trim() === sendError.question)}
                      disabled={!!blocked || busy || !!info?.busy}
                    >
                      <RotateCcw aria-hidden="true" /> Try again
                    </Button>
                  }
                >
                  {sendError.message} {sendError.restored ? "Your question is back in the field below." : `Try again sends “${sendError.question}” again.`}
                </Banner>
              )}
              <Composer
                value={input}
                onChange={setInput}
                onSend={() => void send(input, true)}
                onStop={stop}
                canStop={busy || !!info?.busy}
                stopping={stopping}
                sending={busy}
                problem={blocked}
                fixInSettings={!(problems.noKey && /api key/i.test(blocked ?? ""))}
                inputRef={inputRef}
              />
            </div>
          </div>
        </div>
      </div>
      <p className="sr-only" role="status">
        {announce}
      </p>

      <DeleteDialog conversation={toDelete} onClose={() => setToDelete(null)} onDelete={(c) => void remove(c)} />
      <LinkDialog
        target={link}
        onClose={() => setLink(null)}
        onOpen={(url) => {
          setLink(null);
          chatApi.openLink(url).catch((e) => toast("Couldn’t open the link", { description: errorText(e), tone: "error" }));
        }}
      />
      <MemoryDialog open={memoryOpen} onOpenChange={setMemoryOpen} />
    </>
  );
}

function Welcome({
  prompts,
  providerLabel,
  disabled,
  onAsk,
}: {
  prompts: string[];
  providerLabel: string | null;
  disabled: boolean;
  onAsk: (q: string) => void;
}) {
  return (
    <div className="my-auto flex flex-col gap-6 py-6">
      <div className="flex flex-col gap-2">
        <h2 className="font-display text-title font-semibold">What do you want to know?</h2>
        <p className="max-w-[60ch] text-meta text-muted-foreground">
          Answers are written from your timeline cards and link to the cards they use. Screenshots are never sent; card titles and summaries are sent to{" "}
          {providerLabel ? <span translate="no">{providerLabel}</span> : "the model provider"}. Chat remembers your preferences in Memory.
        </p>
      </div>
      {prompts.length > 0 && (
        <div className="flex flex-col gap-2">
          <h3 className="text-caption text-muted-foreground">Try one of these</h3>
          <ul className="grid grid-cols-2 gap-2">
            {prompts.map((q) => (
              <li key={q}>
                <button
                  type="button"
                  onClick={() => onAsk(q)}
                  disabled={disabled}
                  className="h-full w-full rounded-lg border bg-surface-1 px-3.5 py-3 text-left text-body shadow-xs transition-[border-color,background-color] duration-(--dur-fast) hover:border-input hover:bg-surface-2 disabled:opacity-50 disabled:hover:border-border disabled:hover:bg-surface-1"
                >
                  {q}
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

function ConversationSkeleton() {
  return (
    <div className="flex flex-col gap-6" aria-hidden="true">
      <div className="flex justify-end">
        <Skeleton className="h-9 w-1/2 rounded-2xl" />
      </div>
      <div className="flex flex-col gap-2">
        <Skeleton className="h-3 w-1/3" />
        <Skeleton className="h-4 w-full" />
        <Skeleton className="h-4 w-11/12" />
        <Skeleton className="h-4 w-3/4" />
      </div>
    </div>
  );
}
