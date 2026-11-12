import { useCallback, useEffect, useState } from "react";
import { answerInbox, decisionInbox, listChats, type Decision } from "./api";

/** The cabin's needs-attention words (`motion::needs_attention_summary`). */
export function needsLine(count: number): string {
  if (count === 1) return "1 thing needs a decision";
  return `${count} things need a decision`;
}

/** The decision inbox (Order 733, the cabin's Pactwatch inbox): one line
 * over the chat — how much waits on you — opening into one row per
 * decision. Rows answer through the same path as their cards; an elicit's
 * row opens its chat instead, and its Deny cancels the ask. Hard rows
 * never offer Always. */
export function Inbox({ onOpenChat }: { onOpenChat: (chatId: string) => void }) {
  const [rows, setRows] = useState<Decision[]>([]);
  const [titles, setTitles] = useState<Record<string, string>>({});
  const [open, setOpen] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    // The async boundary keeps a shell without the command (the web app
    // before it pairs, an older build) from throwing in the effect.
    Promise.resolve()
      .then(() => decisionInbox())
      .then(setRows)
      .catch(() => {
        // The inbox is a mirror; a failed read just leaves the last list.
      });
  }, []);

  useEffect(() => {
    refresh();
    const timer = setInterval(refresh, 5000);
    return () => clearInterval(timer);
  }, [refresh]);

  useEffect(() => {
    Promise.resolve()
      .then(() => listChats())
      .then((chats) => setTitles(Object.fromEntries(chats.map((c) => [c.id, c.title]))))
      .catch(() => {});
  }, [rows.length]);

  if (rows.length === 0) return null;

  async function answer(row: Decision, approve: boolean) {
    setError(null);
    try {
      const out = await answerInbox(row.chatId, row.id, row.kind, approve);
      if (out.startsWith("open:")) onOpenChat(out.slice("open:".length));
    } catch (err) {
      setError(String(err));
    }
    refresh();
  }

  function chatLabel(row: Decision): string {
    if (row.kind === "park") return "Desktop session";
    return titles[row.chatId] || "Chat";
  }

  return (
    <div className="inbox" role="region" aria-label="Decision inbox">
      <button
        type="button"
        className="inbox-line"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        {needsLine(rows.length)}. Everything else is on track.
      </button>
      {open && (
        <ul className="inbox-rows">
          {rows.map((row) => (
            <li key={`${row.kind}:${row.id}`} className={row.hard ? "inbox-row hard" : "inbox-row"}>
              <button
                type="button"
                className="inbox-what"
                title={row.what}
                onClick={() => row.chatId && onOpenChat(row.chatId)}
              >
                <span className="inbox-text">{row.what}</span>
                <span className="inbox-chat">{chatLabel(row)}</span>
              </button>
              <span className="inbox-buttons">
                {row.kind === "elicit" ? (
                  <>
                    <button type="button" onClick={() => onOpenChat(row.chatId)}>
                      Open
                    </button>
                    <button
                      type="button"
                      className="quiet danger"
                      onClick={() => void answer(row, false)}
                    >
                      Deny
                    </button>
                  </>
                ) : (
                  <>
                    <button type="button" onClick={() => void answer(row, true)}>
                      Approve
                    </button>
                    <button
                      type="button"
                      className="quiet danger"
                      onClick={() => void answer(row, false)}
                    >
                      Deny
                    </button>
                  </>
                )}
              </span>
            </li>
          ))}
        </ul>
      )}
      {error && (
        <p className="inbox-error" role="alert">
          {error}
        </p>
      )}
    </div>
  );
}
