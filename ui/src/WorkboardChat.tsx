import { useEffect, useState, type FormEvent, type KeyboardEvent } from "react";
import {
  answerPermission,
  cancelChat,
  loadChat,
  onChatEvent,
  sendChat,
  workboardSay,
  type Answer,
  type BoardCard,
  type BoardPage,
  type TranscriptLine,
} from "./api";
import { newConversationId } from "./Chat";

/** How many of the card chat's newest lines the card shows. */
export const CARD_CHAT_LINES = 12;

/** The card's chat when it is one of this app's (the cabin's are not). */
export function ownChat(card: BoardCard): string | null {
  return card.chat?.startsWith("shell-") ? card.chat : null;
}

/** The composer's hint, as the cabin words it. */
export function chatHint(linked: boolean, done: boolean): string {
  if (linked) return "Reply to the agent…";
  if (done) return "Ask the agent about this card…";
  return "Tell the agent how to work on this…";
}

/**
 * A card's own chat: its newest lines, the reply as it streams, Say (Send, or
 * Work on it for a card with no chat yet) and Stop while its turn runs. An
 * ask for your OK shows here with Allow and Deny. Enter sends; Shift+Enter is
 * a new line.
 */
export function CardChat({
  card,
  done,
  onPage,
  onError,
}: {
  card: BoardCard;
  done: boolean;
  onPage: (page: BoardPage) => void;
  onError: (err: unknown) => void;
}) {
  const [started, setStarted] = useState<string | null>(null);
  const chat = ownChat(card) ?? started;
  const [lines, setLines] = useState<TranscriptLine[]>([]);
  const [live, setLive] = useState("");
  const [sending, setSending] = useState(false);
  const [draft, setDraft] = useState("");
  /** What you just said, until the saved chat has it. */
  const [said, setSaid] = useState<string | null>(null);
  const [ask, setAsk] = useState<{ id: string; title: string } | null>(null);
  const [reload, setReload] = useState(0);
  const running = sending || card.running;

  useEffect(() => {
    if (!chat) return;
    let gone = false;
    loadChat(chat, CARD_CHAT_LINES)
      .then((t) => {
        if (!gone) setLines(t.lines);
      })
      .catch(() => {
        // A chat whose first turn has not saved yet has no transcript.
      });
    return () => {
      gone = true;
    };
  }, [chat, reload, card.running]);

  useEffect(() => {
    if (!chat) return;
    let gone = false;
    let unlisten: (() => void) | undefined;
    void onChatEvent(({ conversationId, event }) => {
      if (conversationId !== chat) return;
      if (event.kind === "text") setLive((t) => t + event.text);
      else if (event.kind === "permission") setAsk({ id: event.id, title: event.title });
      else if (event.kind === "answered") setAsk((a) => (a?.id === event.id ? null : a));
      else if (event.kind === "done" || event.kind === "error") {
        setLive("");
        setAsk(null);
        setReload((n) => n + 1);
      }
    }).then((off) => {
      if (gone) off();
      else unlisten = off;
    });
    return () => {
      gone = true;
      unlisten?.();
    };
  }, [chat]);

  async function say(event?: FormEvent) {
    event?.preventDefault();
    const text = draft.trim();
    if ((chat && !text) || running) return;
    try {
      const said = await workboardSay(card.id, text, newConversationId());
      setStarted(said.conversationId);
      onPage(said.page);
      setDraft("");
      setSaid(text || "Work on it");
      setSending(true);
      try {
        await sendChat(said.conversationId, said.message);
      } finally {
        setSending(false);
        setSaid(null);
        setReload((n) => n + 1);
      }
    } catch (err) {
      onError(err);
    }
  }

  function onKey(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void say();
    }
  }

  async function answer(how: Answer) {
    if (!chat || !ask) return;
    try {
      await answerPermission(chat, ask.id, how);
      setAsk(null);
    } catch (err) {
      onError(err);
    }
  }

  return (
    <section className="work-chat" aria-label="Card chat">
      {(lines.length > 0 || said || live) && (
        <ol className="work-chat-lines">
          {lines.map((line, i) => (
            <li key={i} className={`work-chat-line ${line.role}`}>
              {line.text}
            </li>
          ))}
          {said && <li className="work-chat-line user">{said}</li>}
          {live && <li className="work-chat-line assistant">{live}</li>}
        </ol>
      )}
      {running && !live && !ask && <p className="card-status">Working…</p>}
      {ask && (
        <div className="work-chat-ask" role="group" aria-label="Needs your OK">
          <p>Needs your OK: {ask.title}</p>
          <div className="card-buttons">
            <button type="button" onClick={() => void answer("allow")}>
              Allow
            </button>
            <button type="button" className="quiet" onClick={() => void answer("deny")}>
              Deny
            </button>
          </div>
        </div>
      )}
      <form className="work-chat-say" onSubmit={(e) => void say(e)}>
        <textarea
          aria-label="Message the card's agent"
          placeholder={chatHint(Boolean(chat), done)}
          rows={2}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={onKey}
        />
        <div className="card-buttons">
          {running ? (
            <button type="button" disabled={!chat} onClick={() => chat && void cancelChat(chat)}>
              Stop
            </button>
          ) : (
            <button type="submit" disabled={Boolean(chat) && !draft.trim()}>
              {chat ? "Send" : "Work on it"}
            </button>
          )}
        </div>
      </form>
    </section>
  );
}
