import { useCallback, useEffect, useMemo, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import type { ViewProps } from "./types";
import {
  chatApi,
  errorText,
  linkConfirmText,
  providerName,
  type ChatInfo,
  type ChatMemory,
  type ChatMessage,
  type ChatProgress,
  type Citation,
  type Conversation,
} from "./chat-api";
import { ChatMarkdown } from "./ChatMarkdown";
import "./chat.css";

/** The open conversation survives switching tabs. */
let lastConversationId: number | null = null;

interface Step {
  label: string;
  summary?: string;
  ok?: boolean;
}

function shortDay(day: string): string {
  const [y, m, d] = day.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString("en-GB", { weekday: "short", day: "numeric", month: "short" });
}

function clockOf(ts: number): string {
  return new Date(ts * 1000).toLocaleTimeString("en-US", { hour: "numeric", minute: "2-digit" });
}

/** Dayflow's history groups: today, yesterday, this week, earlier. */
function groupConversations(list: Conversation[]): { title: string; items: Conversation[] }[] {
  const now = new Date();
  const startToday = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime() / 1000;
  const startYesterday = startToday - 86400;
  const startWeek = startToday - 6 * 86400;
  const groups = [
    { title: "Today", items: [] as Conversation[] },
    { title: "Yesterday", items: [] as Conversation[] },
    { title: "This week", items: [] as Conversation[] },
    { title: "Earlier", items: [] as Conversation[] },
  ];
  for (const c of list) {
    const t = c.updatedAt;
    groups[t >= startToday ? 0 : t >= startYesterday ? 1 : t >= startWeek ? 2 : 3].items.push(c);
  }
  return groups.filter((g) => g.items.length);
}

function conversationWhen(c: Conversation): string {
  const d = new Date(c.updatedAt * 1000);
  const today = new Date();
  const sameDay = d.toDateString() === today.toDateString();
  const when = sameDay ? clockOf(c.updatedAt) : d.toLocaleDateString("en-GB", { day: "numeric", month: "short" });
  const who = c.model || providerName(c.provider);
  return who ? `${when} · ${who}` : when;
}

function formatTokens(n: number): string {
  return n.toLocaleString("en-US");
}

export function ChatView(props: ViewProps) {
  const [info, setInfo] = useState<ChatInfo | null>(null);
  const [conversations, setConversations] = useState<Conversation[]>([]);
  const [activeId, setActiveId] = useState<number | null>(lastConversationId);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [pending, setPending] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [steps, setSteps] = useState<Step[]>([]);
  const [stage, setStage] = useState<"thinking" | "tools">("thinking");
  const [error, setError] = useState<string | null>(null);
  const [input, setInput] = useState("");
  const [memory, setMemory] = useState<ChatMemory | null>(null);
  const [memoryDraft, setMemoryDraft] = useState("");
  const [modelFormOpen, setModelFormOpen] = useState(false);
  const [modelDraft, setModelDraft] = useState("");
  const [copied, setCopied] = useState<number | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);

  const loadList = useCallback(() => chatApi.list().then(setConversations).catch(() => {}), []);
  const memoryRef = useRef<ChatMemory | null>(null);
  const busyRef = useRef(false);
  const loadMemory = useCallback(
    (resetDraft: boolean) =>
      chatApi
        .getMemory()
        .then((m) => {
          const old = memoryRef.current;
          // Keep an unsaved edit unless asked to reset.
          setMemoryDraft((d) => (resetDraft || old === null || d === old.content ? m.content : d));
          memoryRef.current = m;
          setMemory(m);
        })
        .catch(() => {}),
    [],
  );

  const openConversation = useCallback((id: number | null) => {
    lastConversationId = id;
    setActiveId(id);
    setError(null);
    if (id === null) {
      setMessages([]);
      return;
    }
    chatApi
      .get(id)
      .then((d) => setMessages(d.messages))
      .catch((e) => {
        lastConversationId = null;
        setActiveId(null);
        setMessages([]);
        setError(errorText(e));
      });
  }, []);

  // Provider settings may change in the Settings view.
  useEffect(() => {
    chatApi.info().then(setInfo).catch((e) => setError(errorText(e)));
  }, [props.settings]);

  useEffect(() => {
    loadList();
    loadMemory(true);
    if (lastConversationId !== null) openConversation(lastConversationId);
  }, [loadList, loadMemory, openConversation]);

  // If a question was still being answered when this view was left, wait for it, then show
  // the conversation that was updated last.
  useEffect(() => {
    if (!info?.busy || busy) return;
    const t = setInterval(async () => {
      const i = await chatApi.info().catch(() => null);
      if (i && !i.busy) {
        clearInterval(t);
        setInfo(i);
        const list = await chatApi.list().catch(() => [] as Conversation[]);
        setConversations(list);
        if (list.length) openConversation(list[0].id);
      }
    }, 1500);
    return () => clearInterval(t);
  }, [info?.busy, busy, openConversation]);

  // One listener for the view's lifetime; events only count while this view asked a question.
  useEffect(() => {
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
      un.then((f) => f());
    };
  }, []);

  useEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [messages, pending, steps, busy]);

  const send = async (raw: string) => {
    const text = raw.trim();
    if (!text || busy || info?.busy) return;
    setError(null);
    setPending(text);
    setInput("");
    setSteps([]);
    setStage("thinking");
    setBusy(true);
    busyRef.current = true;
    try {
      const d = await chatApi.send(activeId, text);
      lastConversationId = d.conversation.id;
      setActiveId(d.conversation.id);
      setMessages(d.messages);
      loadList();
      loadMemory(false);
    } catch (e) {
      // Nothing was stored (for example no API key); give the question back.
      setError(errorText(e));
      setInput(text);
    } finally {
      busyRef.current = false;
      setPending(null);
      setBusy(false);
      chatApi.info().then(setInfo).catch(() => {});
      inputRef.current?.focus();
    }
  };

  const onSubmit = (e: FormEvent) => {
    e.preventDefault();
    send(input);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      send(input);
    }
  };

  const openLink = (url: string) => {
    const question = linkConfirmText(url);
    if (!question || !window.confirm(question)) return;
    chatApi.openLink(url).catch((e) => setError(errorText(e)));
  };

  const remove = async (c: Conversation) => {
    if (busy || c.id === info?.busyConversationId) return;
    if (!window.confirm(`Delete the chat "${c.title}"?`)) return;
    try {
      await chatApi.remove(c.id);
      if (c.id === activeId) openConversation(null);
      loadList();
    } catch (e) {
      setError(errorText(e));
    }
  };

  const saveModel = async (e: FormEvent) => {
    e.preventDefault();
    try {
      setInfo(await chatApi.setModel(modelDraft));
      setModelFormOpen(false);
    } catch (err) {
      setError(errorText(err));
    }
  };

  const saveMemory = async (content: string) => {
    try {
      const m = await chatApi.saveMemory(content);
      memoryRef.current = m;
      setMemory(m);
      setMemoryDraft(m.content);
    } catch (e) {
      setError(errorText(e));
    }
  };

  const copy = async (m: ChatMessage) => {
    try {
      await navigator.clipboard.writeText(m.content);
      setCopied(m.id);
      setTimeout(() => setCopied((c) => (c === m.id ? null : c)), 1500);
    } catch {
      setError("Could not copy to the clipboard.");
    }
  };

  const openCitation = (c: Citation) => props.openTimeline(c.day, c.cardId ?? undefined);

  const lastAssistant = [...messages].reverse().find((m) => m.role !== "user");
  const suggestions = !busy && lastAssistant?.role === "assistant" ? lastAssistant.suggestions : [];
  const groups = useMemo(() => groupConversations(conversations), [conversations]);
  const empty = messages.length === 0 && pending === null;
  const blocked = info?.problem ?? null;
  const memoryDirty = memory !== null && memoryDraft !== memory.content;

  return (
    <section className="chat" aria-label="Chat">
      <aside className="chat-side" aria-label="Chat history">
        <button className="primary chat-new" onClick={() => openConversation(null)} disabled={busy}>
          New chat
        </button>
        <h2 className="chat-side-title">History</h2>
        {conversations.length === 0 ? (
          <p className="muted chat-side-empty">No saved chats yet</p>
        ) : (
          groups.map((g) => (
            <div key={g.title} className="chat-group">
              <h3>{g.title}</h3>
              <ul className="chat-convs">
                {g.items.map((c) => (
                  <li key={c.id} className={c.id === activeId ? "chat-conv on" : "chat-conv"}>
                    <button className="chat-conv-open" onClick={() => openConversation(c.id)} disabled={busy} aria-current={c.id === activeId ? "true" : undefined}>
                      <span className="chat-conv-title">{c.title}</span>
                      <span className="chat-conv-when">{conversationWhen(c)}</span>
                    </button>
                    <button
                      className="chat-conv-delete"
                      onClick={() => remove(c)}
                      disabled={busy || c.id === info?.busyConversationId}
                      aria-label={`Delete conversation: ${c.title}`}
                      title={c.id === info?.busyConversationId ? "This chat is still being answered" : "Delete conversation"}
                    >
                      ×
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          ))
        )}

        <details className="chat-memory">
          <summary>Memory</summary>
          <p className="chat-hint">Updated automatically from assistant replies. You can edit it.</p>
          <label className="chat-memory-field">
            <span className="chat-sr">Memory text</span>
            <textarea
              rows={6}
              value={memoryDraft}
              maxLength={memory?.maxChars}
              onChange={(e) => setMemoryDraft(e.target.value)}
              placeholder={"Profile: …\nStyle: …"}
            />
          </label>
          <p className="chat-hint">
            {memoryDraft.length}/{memory?.maxChars ?? 10000} · Last updated:{" "}
            {memory?.updatedAt ? new Date(memory.updatedAt * 1000).toLocaleString("en-GB", { day: "numeric", month: "short", hour: "2-digit", minute: "2-digit" }) : "Not saved yet"}
          </p>
          <div className="chat-memory-actions">
            <button className="ghost" disabled={!memoryDirty} onClick={() => saveMemory(memoryDraft)}>
              Save
            </button>
            <button className="ghost" disabled={!memoryDirty} onClick={() => loadMemory(true)}>
              Reload
            </button>
            <button className="ghost" disabled={!memory?.content} onClick={() => saveMemory("")}>
              Clear
            </button>
          </div>
        </details>
      </aside>

      <div className="chat-main">
        <header className="chat-head">
          {info && (
            <p className="chat-model">
              Answers by <strong>{info.providerLabel}</strong>, model <code>{info.model}</code>
              {info.chatModel ? " (chat only)" : " (same as the timeline)"}{" "}
              <button
                className="link"
                onClick={() => {
                  setModelDraft(info.chatModel);
                  setModelFormOpen((o) => !o);
                }}
                aria-expanded={modelFormOpen}
              >
                Change model
              </button>
            </p>
          )}
          {info && modelFormOpen && (
            <form className="chat-model-form" onSubmit={saveModel}>
              <label>
                <span>Chat model</span>
                <input value={modelDraft} onChange={(e) => setModelDraft(e.target.value)} placeholder={info.timelineModel} />
              </label>
              <button className="primary" type="submit">
                Save
              </button>
              <span className="chat-hint">Leave empty to use the timeline model ({info.timelineModel}). The provider is set in Settings.</span>
            </form>
          )}
        </header>

        {blocked && (
          <p className="chat-problem" role="alert">
            {blocked}
          </p>
        )}

        <div className="chat-scroll" ref={scrollRef}>
          {empty ? (
            <div className="chat-welcome">
              <h2>Ask about your Daytrace data</h2>
              <p>Ask questions and analyze your timeline. Answers link to the cards they come from.</p>
              <p className="chat-hint">I remember your response preferences, so feel free to teach me your style.</p>
              <h3>Try one of these</h3>
              <ul className="chat-welcome-list">
                {(info?.welcomePrompts ?? []).map((q) => (
                  <li key={q}>
                    <button className="chat-welcome-prompt" onClick={() => send(q)} disabled={busy || !!blocked}>
                      {q}
                    </button>
                  </li>
                ))}
              </ul>
            </div>
          ) : (
            <ol className="chat-log">
              {messages.map((m) => (
                <MessageItem key={m.id} m={m} copied={copied === m.id} onCopy={() => copy(m)} onCitation={openCitation} onLink={openLink} />
              ))}
              {pending !== null && (
                <li className="chat-msg chat-msg-user">
                  <p className="chat-user-text">{pending}</p>
                </li>
              )}
            </ol>
          )}
        </div>

        {!busy && info?.busy && (
          <div className="chat-status" role="status" aria-live="polite">
            <div className="chat-status-head">
              <span className="chat-status-stage">Still answering an earlier question</span>
              <button className="ghost" onClick={() => chatApi.cancel()}>
                Stop
              </button>
            </div>
          </div>
        )}

        {busy && (
          <div className="chat-status" role="status" aria-live="polite">
            <div className="chat-status-head">
              <span className="chat-status-stage">{stage === "tools" ? "Running tools" : "Thinking"}</span>
              <button className="ghost" onClick={() => chatApi.cancel()}>
                Stop
              </button>
            </div>
            {steps.length > 0 && (
              <ul className="chat-steps">
                {steps.map((s, i) => (
                  <li key={i} className={s.ok === false ? "chat-step chat-step-failed" : s.summary ? "chat-step chat-step-done" : "chat-step"}>
                    {s.summary ?? `${s.label}…`}
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}

        {error && (
          <p className="chat-error" role="alert">
            {error}
          </p>
        )}

        {suggestions.length > 0 && (
          <div className="chat-suggestions">
            <h3>Follow up</h3>
            <ul>
              {suggestions.map((s) => (
                <li key={s}>
                  <button className="chat-chip" onClick={() => send(s)} disabled={!!blocked}>
                    {s}
                  </button>
                </li>
              ))}
            </ul>
          </div>
        )}

        <form className="chat-composer" onSubmit={onSubmit}>
          <label className="chat-sr" htmlFor="chat-input">
            Question
          </label>
          <textarea
            id="chat-input"
            ref={inputRef}
            rows={2}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={onKeyDown}
            placeholder="Ask about your Daytrace data"
            disabled={!!blocked}
          />
          <button className="primary" type="submit" disabled={busy || !!info?.busy || !input.trim() || !!blocked}>
            {busy ? "Answering" : "Send"}
          </button>
        </form>
      </div>
    </section>
  );
}

function MessageItem({
  m,
  copied,
  onCopy,
  onCitation,
  onLink,
}: {
  m: ChatMessage;
  copied: boolean;
  onCopy: () => void;
  onCitation: (c: Citation) => void;
  onLink: (url: string) => void;
}) {
  if (m.role === "user") {
    return (
      <li className="chat-msg chat-msg-user">
        <p className="chat-user-text">{m.content}</p>
      </li>
    );
  }
  const byId = new Map(m.citations.map((c) => [c.citedId, c]));
  const tools = m.tools.length > 0 && (
    <details className="chat-tools">
      <summary>
        Looked at {m.tools.length} {m.tools.length === 1 ? "source" : "sources"}
      </summary>
      <ul>
        {m.tools.map((t, i) => (
          <li key={i} className={t.ok ? "chat-tool" : "chat-tool chat-tool-failed"}>
            {t.ok ? t.summary : `${t.label}: ${t.summary}`}
          </li>
        ))}
      </ul>
    </details>
  );
  const meta = [m.provider ? providerName(m.provider) : "", m.model ?? ""].filter(Boolean).join(" · ");
  const tokens = m.inputTokens || m.outputTokens ? `${formatTokens(m.inputTokens ?? 0)} tokens in, ${formatTokens(m.outputTokens ?? 0)} out` : "";

  if (m.role === "error") {
    return (
      <li className="chat-msg chat-msg-error" role="alert">
        {tools}
        <p>{m.content}</p>
        {m.detail && (
          <details className="chat-detail">
            <summary>Technical details</summary>
            <pre>{m.detail}</pre>
          </details>
        )}
        {meta && <p className="chat-meta">{meta}</p>}
      </li>
    );
  }
  return (
    <li className="chat-msg chat-msg-assistant">
      {tools}
      <ChatMarkdown
        text={m.content}
        onExternalLink={onLink}
        renderCard={(id, label, key) => {
          const c = byId.get(id);
          if (!c) return null;
          return (
            <button key={key} className="chat-cite" onClick={() => onCitation(c)} title={`Open "${c.title}" on the timeline`}>
              {label}
            </button>
          );
        }}
      />
      {m.citations.length > 0 && (
        <div className="chat-sources">
          <h4>Cards</h4>
          <ul>
            {m.citations.map((c) => (
              <li key={c.citedId}>
                <button className="chat-source" onClick={() => onCitation(c)}>
                  <span className="chat-source-title">{c.title}</span>
                  <span className="chat-source-when">
                    {shortDay(c.day)}, {c.start} to {c.end}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
      <footer className="chat-meta">
        {meta && <span>{meta}</span>}
        {tokens && <span>{tokens}</span>}
        <button className="link" onClick={onCopy}>
          {copied ? "Copied" : "Copy answer"}
        </button>
      </footer>
    </li>
  );
}
