import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import {
  answerPermission,
  cancelChat,
  closeChat,
  onChatEvent,
  sendChat,
  type Answer,
  type ChatEvent,
} from "./api";

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
    };

function newConversationId(): string {
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
    default:
      return items;
  }
}

/** The turn ended: stop streaming, and retire cards nobody answered. */
function settle(items: Item[]): Item[] {
  return items.map((item) => {
    if (item.kind === "assistant" && item.streaming) return { ...item, streaming: false };
    if (item.kind === "permission" && item.answer === null) return { ...item, answer: "ended" };
    return item;
  });
}

const ANSWERED: Record<Answer | "ended", string> = {
  allow: "Allowed once",
  always: "Always allowed",
  deny: "Denied",
  ended: "Not run: the turn ended",
};

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
        GrokHub wants to run <code>{item.title}</code>
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

export function Chat({ onOpenSettings }: { onOpenSettings: () => void }) {
  const [conversationId, setConversationId] = useState(newConversationId);
  const [items, setItems] = useState<Item[]>([]);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const busyRef = useRef(false);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let dropped = false;
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
    return () => {
      dropped = true;
      stop?.();
    };
  }, [conversationId]);

  useEffect(() => {
    endRef.current?.scrollIntoView?.({ block: "end" });
  }, [items]);

  async function send(event?: FormEvent) {
    event?.preventDefault();
    const text = draft.trim();
    if (!text || busy) return;
    setItems((now) => [...now, { kind: "user", text }]);
    setDraft("");
    setError(null);
    busyRef.current = true;
    setBusy(true);
    try {
      await sendChat(conversationId, text);
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
    !(last?.kind === "permission" && last.answer === null);

  return (
    <section className="chat" aria-label="Chat">
      <div className="log" role="log" aria-live="polite">
        {items.length === 0 && !busy && <p className="empty">Ask Grok anything.</p>}
        {items.map((item, i) => {
          switch (item.kind) {
            case "user":
              return (
                <div key={i} className="bubble user" data-role="user">
                  {item.text}
                </div>
              );
            case "assistant":
              return (
                <div
                  key={i}
                  className={`bubble assistant${item.streaming ? " streaming" : ""}`}
                  data-role="assistant"
                >
                  {item.text}
                </div>
              );
            case "tool":
              return (
                <div key={i} className="card tool" data-status={item.status}>
                  <span className="card-title">{item.title}</span>
                  <span className="card-status">{item.status}</span>
                  {item.detail && <pre className="card-detail">{item.detail}</pre>}
                </div>
              );
            case "permission":
              return (
                <PermissionCard key={i} item={item} onAnswer={(a) => void answer(item.id, a)} />
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
      <form className="composer" onSubmit={send}>
        <textarea
          aria-label="Message"
          placeholder="Message Grok"
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
