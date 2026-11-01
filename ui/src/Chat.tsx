import { memo, useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import {
  answerElicit,
  answerPermission,
  cancelChat,
  closeChat,
  getSettings,
  onChatEvent,
  onEventGap,
  saveSettings,
  sendChat,
  type Answer,
  type Approvals,
  type ChatEvent,
  type ElicitAction,
  type SessionMode,
  type Settings,
  type Suggestion,
} from "./api";
import type { RunNotice } from "./Automations";
import { ElicitCard, ModeChips, type ElicitItem } from "./Modes";
import { PRODUCT_NAME } from "./brand";
import { onArrowFocus, usePeek } from "./peek";

export type Item =
  | { kind: "user"; text: string }
  | { kind: "assistant"; text: string; streaming: boolean }
  | { kind: "tool"; id: string; title: string; status: string; detail: string }
  | {
      kind: "permission";
      id: string;
      title: string;
      action: string;
      reason: string;
      /** "ended": the turn stopped before anyone answered, so nothing ran. */
      answer: Answer | "ended" | null;
    }
  | { kind: "suggest"; items: Suggestion[] }
  | ({ kind: "notice" } & RunNotice)
  | ElicitItem;

/** How much of a run's reply its notice shows; Automations has the rest. */
export const NOTICE_CHARS = 280;

/** At most this many follow-ups show under a reply. */
export const SUGGESTIONS_MAX = 3;

/** The next steps under a reply: one row that scrolls sideways when it runs
 * out of room, with the next chip peeking in. Left and Right move along it. */
function SuggestRow({
  items,
  busy,
  onPick,
}: {
  items: Suggestion[];
  busy: boolean;
  onPick: (prompt: string) => void;
}) {
  const row = useRef<HTMLDivElement>(null);
  usePeek(row, "x", items.length);
  return (
    <div className="card suggest" role="group" aria-label="Suggestions">
      <div ref={row} className="card-buttons" onKeyDown={(e) => onArrowFocus(e, "x")}>
        {items.map((s) => (
          <button
            key={s.label}
            type="button"
            className="quiet"
            title={s.prompt}
            disabled={busy}
            onClick={() => onPick(s.prompt)}
          >
            {s.label}
          </button>
        ))}
      </div>
    </div>
  );
}

export interface Prefill {
  text: string;
  n: number;
  conversationId?: string;
}

const PLACEHOLDERS: Record<SessionMode, string> = {
  chat: "Message Grok",
  plan: "Describe what to plan",
  btw: "Ask on the side",
};

export const CHAT_EMPTY = "Ask Grok anything.";
export const SIGNED_OUT =
  `${PRODUCT_NAME} chats with your xAI API key. It's kept in this device's keychain, never in a file.`;
export const SIGNED_OUT_KEY_HINT = "No key yet? Make one at console.x.ai.";
export const SIGNED_OUT_REMOTE =
  `${PRODUCT_NAME} on your computer has no API key yet. Add one there, then chat here.`;

/** One primary sign-in button and one hint, on every platform. The web app
 * signs in on the computer, so it only says so. */
function SignedOut({ remote, onSignIn }: { remote: boolean; onSignIn: () => void }) {
  if (remote) return <p className="empty">{SIGNED_OUT_REMOTE}</p>;
  return (
    <div className="signed-out" role="region" aria-label="Sign in">
      <h2>Sign in to start</h2>
      <p>{SIGNED_OUT}</p>
      <button type="button" onClick={onSignIn}>
        Add your API key
      </button>
      <p className="note">{SIGNED_OUT_KEY_HINT}</p>
    </div>
  );
}

export const EVENTS_LOST = `Lost touch with ${PRODUCT_NAME} on your computer for a while. Part of this reply may be missing.`;
export const MISSION_BUSY = "A reply is still running. Stop it or wait, then press To mission again.";

export function newConversationId(): string {
  return `shell-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

/** Folds one engine event into the log. Errors and Done are handled by the caller. */
export function applyEvent(items: Item[], event: ChatEvent): Item[] {
  switch (event.kind) {
    case "text": {
      const last = items[items.length - 1];
      if (last?.kind === "assistant" && last.streaming) {
        return [...items.slice(0, -1), { ...last, text: last.text + event.text }];
      }
      return [...items, { kind: "assistant", text: event.text, streaming: true }];
    }
    case "tool": {
      const card: Item = { ...event };
      const at = items.findIndex((item) => item.kind === "tool" && item.id === event.id);
      if (at < 0) return [...items, card];
      return items.map((item, i) => (i === at ? card : item));
    }
    case "permission":
      return [...items, { ...event, answer: null }];
    // Deny on a notification: the card shows what ran, unless the turn already ended.
    case "answered":
      return items.map((item) =>
        item.kind === "permission" && item.id === event.id && item.answer !== "ended"
          ? { ...item, answer: event.answer }
          : item,
      );
    // The plan comes again on its approval card; one copy is enough.
    case "elicit":
      return [...items, { ...event, answer: null }];
    case "suggestions":
      if (event.items.length === 0) return items;
      return [...items, { kind: "suggest", items: event.items.slice(0, SUGGESTIONS_MAX) }];
    default:
      return items;
  }
}

/** The turn ended: stop streaming, and retire cards nobody answered. */
function settle(items: Item[]): Item[] {
  return items.map((item) => {
    if (item.kind === "assistant" && item.streaming) return { ...item, streaming: false };
    if (item.kind === "permission" && item.answer === null) return { ...item, answer: "ended" };
    if (item.kind === "elicit" && item.answer === null) return { ...item, answer: "ended" };
    return item;
  });
}

const ANSWERED: Record<Answer | "ended", string> = {
  allow: "Allowed once",
  always: "Always allowed",
  deny: "Denied",
  ended: "Not run: the turn ended",
};

// Typing in the composer re-renders Chat, and a reply streaming in re-renders it
// on every chunk. These take plain values and are memoized, so only the message
// that changed repaints and a long conversation stays cheap to type under.
const UserBubble = memo(function UserBubble({ text }: { text: string }) {
  return (
    <div className="bubble user" data-role="user">
      {text}
    </div>
  );
});

const AssistantBubble = memo(function AssistantBubble({
  text,
  streaming,
}: {
  text: string;
  streaming: boolean;
}) {
  return (
    <div className={`bubble assistant${streaming ? " streaming" : ""}`} data-role="assistant">
      {text}
    </div>
  );
});

const ToolCard = memo(function ToolCard({
  title,
  status,
  detail,
}: {
  title: string;
  status: string;
  detail?: string;
}) {
  return (
    <div className="card tool" data-status={status}>
      <span className="card-title">{title}</span>
      <span className="card-status">{status}</span>
      {detail && <pre className="card-detail">{detail}</pre>}
    </div>
  );
});

function PermissionCard({
  item,
  onAnswer,
}: {
  item: Extract<Item, { kind: "permission" }>;
  onAnswer: (answer: Answer) => void;
}) {
  return (
    <div className="card permission" role="group" aria-label={`Approve ${item.title}`}>
      <div className="card-title">
        {PRODUCT_NAME} wants to run <code>{item.title}</code>
      </div>
      {item.action && <pre className="card-action">{item.action}</pre>}
      {item.reason && <p className="card-reason">{item.reason}</p>}
      {item.answer ? (
        <p className="card-answer">{ANSWERED[item.answer]}</p>
      ) : (
        <div className="card-buttons">
          <button type="button" onClick={() => onAnswer("allow")}>
            Allow once
          </button>
          <button type="button" className="quiet" onClick={() => onAnswer("always")}>
            Always allow
          </button>
          <button type="button" className="quiet danger" onClick={() => onAnswer("deny")}>
            Deny
          </button>
        </div>
      )}
    </div>
  );
}

/** `prefill`: text another page put in the box (Pulse's Run, Journal's To
 * mission). A new `n` replaces the draft; nothing is sent until the person
 * sends it. With `conversationId` the chat moves to that conversation first,
 * like Start over, unless it is already there. `active`: the Chat tab is
 * showing, so the approvals chips read Settings again. `remote`: the web app,
 * where the approvals stay the computer's. `notice`: an automation run that
 * ended; a new `n` posts it in this chat. */
export function Chat({
  onOpenSettings,
  onSignIn = onOpenSettings,
  onOpenAutomations,
  prefill,
  notice,
  active = true,
  remote = false,
}: {
  onOpenSettings: () => void;
  /** The signed-out card's button: Settings → Account. */
  onSignIn?: () => void;
  onOpenAutomations?: () => void;
  prefill?: Prefill;
  notice?: RunNotice & { n: number };
  active?: boolean;
  remote?: boolean;
}) {
  const [conversationId, setConversationId] = useState(newConversationId);
  const [items, setItems] = useState<Item[]>([]);
  const [draft, setDraft] = useState("");

  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const busyRef = useRef(false);
  const [mode, setMode] = useState<SessionMode>("chat");
  const [settings, setSettings] = useState<Settings | null>(null);

  useEffect(() => {
    if (active) getSettings().then(setSettings, () => {});
  }, [active]);

  async function setApprovals(approvals: Approvals) {
    if (!settings) return;
    try {
      setSettings(
        await saveSettings(
          settings.model,
          settings.closeToTray,
          settings.workspace,
          approvals,
          settings.desktopControl,
        ),
      );
    } catch (err) {
      setError(String(err));
    }
  }

  useEffect(() => {
    if (!prefill) return;
    const to = prefill.conversationId;
    if (to && to !== conversationId) {
      if (busyRef.current) {
        setError(MISSION_BUSY);
        return;
      }
      void closeChat(conversationId);
      setItems([]);
      setError(null);
      setConversationId(to);
    }
    setDraft(prefill.text);
    // Only a new prefill acts; switching conversations must not re-run it.
  }, [prefill]);
  useEffect(() => {
    if (!notice) return;
    const { n: _n, ...run } = notice;
    setItems((now) => [...now, { kind: "notice", ...run }]);
  }, [notice]);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let dropped = false;
    let stopGap: (() => void) | undefined;
    void onChatEvent(({ conversationId: id, event }) => {
      // An event that lands after the turn ended must not reopen it.
      if (id !== conversationId || !busyRef.current) return;
      if (event.kind === "error") setError(event.message);
      else if (event.kind === "done") setItems(settle);
      else setItems((now) => applyEvent(now, event));
    }).then((unlisten) => {
      if (dropped) unlisten();
      else stop = unlisten;
    });
    void onEventGap(() => {
      if (busyRef.current) setError(EVENTS_LOST);
    }).then((unlisten) => {
      if (dropped) unlisten();
      else stopGap = unlisten;
    });
    return () => {
      dropped = true;
      stop?.();
      stopGap?.();
    };
  }, [conversationId]);

  useEffect(() => {
    endRef.current?.scrollIntoView?.({ block: "end" });
  }, [items]);

  /** Sends the draft, or `picked` from a suggestion (the draft stays). */
  async function send(event?: FormEvent, picked?: string) {
    event?.preventDefault();
    const text = (picked ?? draft).trim();
    if (!text || busy) return;
    // Suggestions belong to the reply they followed.
    setItems((now) => [...now.filter((item) => item.kind !== "suggest"), { kind: "user", text }]);
    if (picked === undefined) setDraft("");
    setError(null);
    busyRef.current = true;
    setBusy(true);
    try {
      await sendChat(conversationId, text, mode);
    } catch (err) {
      setError(String(err));
    } finally {
      busyRef.current = false;
      setBusy(false);
      setItems(settle);
    }
  }

  async function answer(id: string, choice: Answer) {
    setItems((now) =>
      now.map((item) =>
        item.kind === "permission" && item.id === id ? { ...item, answer: choice } : item,
      ),
    );
    try {
      await answerPermission(conversationId, id, choice);
    } catch (err) {
      setError(String(err));
    }
  }

  async function answerCard(item: ElicitItem, action: ElicitAction, value: string) {
    setItems((now) =>
      now.map((it) => (it.kind === "elicit" && it.id === item.id ? { ...it, answer: action } : it)),
    );
    // An approved plan leaves plan mode, as in the cabin.
    if (item.server === "Plan" && action === "accept") setMode("chat");
    try {
      await answerElicit(conversationId, item.id, action, item.fieldName, value);
    } catch (err) {
      setError(String(err));
    }
  }

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void send();
    }
  }

  function startOver() {
    if (busy) return;
    void closeChat(conversationId);
    setItems([]);
    setError(null);
    setConversationId(newConversationId());
  }

  // The "…" bubble shows only while nothing else in the log is moving.
  const last = items[items.length - 1];
  const waiting =
    !(last?.kind === "assistant" && last.streaming) &&
    !(last?.kind === "permission" && last.answer === null) &&
    !(last?.kind === "elicit" && last.answer === null);

  return (
    <section className="chat" aria-label="Chat">
      {/* Focusable so the keyboard can scroll a long conversation. */}
      <div className="log" role="log" aria-live="polite" tabIndex={0}>
        {items.length === 0 && !busy && settings && !settings.hasApiKey ? (
          <SignedOut remote={remote} onSignIn={onSignIn} />
        ) : (
          items.length === 0 && !busy && <p className="empty">{CHAT_EMPTY}</p>
        )}
        {items.map((item, i) => {
          switch (item.kind) {
            case "user":
              return <UserBubble key={i} text={item.text} />;
            case "assistant":
              return <AssistantBubble key={i} text={item.text} streaming={Boolean(item.streaming)} />;
            case "tool":
              return (
                <ToolCard key={i} title={item.title} status={item.status} detail={item.detail} />
              );
            case "permission":
              return (
                <PermissionCard key={i} item={item} onAnswer={(a) => void answer(item.id, a)} />
              );
            case "elicit":
              return (
                <ElicitCard
                  key={i}
                  item={item}
                  onAnswer={(action, value) => void answerCard(item, action, value)}
                />
              );
            case "notice":
              return (
                <div
                  key={i}
                  className={item.ok ? "card notice" : "card notice warn"}
                  role="status"
                  aria-label={`${item.name} finished`}
                >
                  <span className="card-title">{item.name}</span>
                  <span className="card-status">{item.ok ? "Finished" : "Failed"}</span>
                  <p className="card-detail-line">
                    {item.text.length > NOTICE_CHARS
                      ? `${item.text.slice(0, NOTICE_CHARS).trimEnd()}…`
                      : item.text}
                  </p>
                  <div className="card-buttons">
                    <button
                      type="button"
                      className="quiet"
                      onClick={() =>
                        setDraft(
                          item.ok
                            ? `About the "${item.name}" automation run: `
                            : `The "${item.name}" automation failed: ${item.text}. What went wrong and how do I fix it?`,
                        )
                      }
                    >
                      {item.ok ? "Ask about it" : "Find the cause"}
                    </button>
                    {onOpenAutomations && (
                      <button type="button" className="link" onClick={onOpenAutomations}>
                        Open Automations
                      </button>
                    )}
                  </div>
                </div>
              );
            case "suggest":
              return (
                <SuggestRow
                  key={i}
                  items={item.items}
                  busy={busy}
                  onPick={(prompt) => void send(undefined, prompt)}
                />
              );
          }
        })}
        {busy && waiting && (
          <div className="bubble assistant streaming" data-role="assistant">
            …
          </div>
        )}
        <div ref={endRef} />
      </div>
      {error && (
        <div className="error" role="alert">
          <span>{error}</span>
          <button type="button" className="link" onClick={onOpenSettings}>
            Open Settings
          </button>
        </div>
      )}
      <ModeChips
        mode={mode}
        onMode={setMode}
        approvals={settings?.approvals ?? null}
        onApprovals={(a) => void setApprovals(a)}
        busy={busy}
        remote={remote}
      />
      <form className="composer" onSubmit={send}>
        <textarea
          aria-label="Message"
          placeholder={PLACEHOLDERS[mode]}
          value={draft}
          rows={2}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={onKeyDown}
        />
        {busy ? (
          <button type="button" onClick={() => void cancelChat(conversationId)}>
            Stop
          </button>
        ) : (
          <button type="submit" disabled={!draft.trim()}>
            Send
          </button>
        )}
        <button type="button" className="quiet" onClick={startOver} disabled={busy}>
          New chat
        </button>
      </form>
    </section>
  );
}
