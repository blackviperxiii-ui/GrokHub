import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { cancelChat, onChatDelta, sendChat, type ChatMessage } from "./api";

function newConversationId(): string {
  return `shell-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 8)}`;
}

export function Chat({ onOpenSettings }: { onOpenSettings: () => void }) {
  const [conversationId, setConversationId] = useState(newConversationId);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const endRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let stop: (() => void) | undefined;
    let dropped = false;
    void onChatDelta((delta) => {
      if (delta.conversationId !== conversationId) return;
      setPending((text) => (text ?? "") + delta.text);
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
  }, [messages, pending]);

  const busy = pending !== null;

  async function send(event?: FormEvent) {
    event?.preventDefault();
    const text = draft.trim();
    if (!text || busy) return;
    const history: ChatMessage[] = [...messages, { role: "user", text }];
    setMessages(history);
    setDraft("");
    setError(null);
    setPending("");
    try {
      const reply = await sendChat(conversationId, history);
      setMessages([...history, { role: "assistant", text: reply }]);
    } catch (err) {
      setError(String(err));
    } finally {
      setPending(null);
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
    setMessages([]);
    setError(null);
    setConversationId(newConversationId());
  }

  return (
    <section className="chat" aria-label="Chat">
      <div className="log" role="log" aria-live="polite">
        {messages.length === 0 && !busy && <p className="empty">Ask Grok anything.</p>}
        {messages.map((message, i) => (
          <div key={i} className={`bubble ${message.role}`} data-role={message.role}>
            {message.text}
          </div>
        ))}
        {busy && (
          <div className="bubble assistant streaming" data-role="assistant">
            {pending || "…"}
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
