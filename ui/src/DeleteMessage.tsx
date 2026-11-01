// Delete one message from a chat: a bubble's ··· button and right-click open
// the same small menu, and the confirm says it's gone for good. An ask goes
// alone; a reply goes whole, with the tools it ran.
import { useEffect, useRef } from "react";
import { PRODUCT_NAME } from "./brand";
import { useDismiss } from "./ui";

export const DELETE_MESSAGE_LABEL = "Delete message…";
/** The ··· button's name for screen readers. The UI font has no "⋯". */
export const MESSAGE_MENU_LABEL = "Message options";
export const DELETE_ASK_QUESTION = "Delete this message?";
export const DELETE_REPLY_QUESTION = "Delete this reply?";
export const DELETE_WARNING = `It's removed from this chat and from the history ${PRODUCT_NAME} reads back. This can't be undone.`;
/** A reply whose ask is still above Show earlier: which ask it answers isn't known yet. */
export const DELETE_NEEDS_EARLIER = "Show earlier messages first, then delete this reply.";

/** What deleting one bubble takes: log items `from..to`, and what the saved
 * chat is told. `text` is the ask (or, for a reply, the ask it answers;
 * `null` before any ask), `later` how often that ask was sent again after. */
export interface Cut {
  from: number;
  to: number;
  reply: boolean;
  text: string | null;
  later: number;
}

/** The log kinds a reply is made of. Run notices and slash results are the
 * app's own and stay. */
const REPLY_KINDS = new Set(["assistant", "tools", "tool", "permission", "elicit", "suggest"]);

type LogLine = { kind: string; text?: string };

function squash(text: string): string {
  return text.split(/\s+/).filter(Boolean).join(" ");
}

/** The cut for bubble `at`, or null when it can't be told yet: a streaming
 * reply, or a reply whose ask hasn't been loaded (`earlier` lines hidden). */
export function cutFor(items: readonly LogLine[], at: number, earlier: number): Cut | null {
  const item = items[at];
  if (!item || (item.kind !== "user" && !REPLY_KINDS.has(item.kind))) return null;
  const laterOf = (i: number) => {
    const want = squash(items[i].text ?? "");
    return items.slice(i + 1).filter((it) => it.kind === "user" && squash(it.text ?? "") === want).length;
  };
  if (item.kind === "user") {
    return { from: at, to: at + 1, reply: false, text: item.text ?? "", later: laterOf(at) };
  }
  let ask = at - 1;
  while (ask >= 0 && items[ask].kind !== "user") ask--;
  if (ask < 0 && earlier > 0) return null;
  let next = at + 1;
  while (next < items.length && items[next].kind !== "user") next++;
  return {
    from: ask + 1,
    to: next,
    reply: true,
    text: ask >= 0 ? (items[ask].text ?? "") : null,
    later: ask >= 0 ? laterOf(ask) : 0,
  };
}

/** The log once `cut` is gone. A reply's run notices and slash results stay. */
export function withoutCut<T extends LogLine>(items: readonly T[], cut: Cut): T[] {
  return items.filter(
    (item, i) => i < cut.from || i >= cut.to || (cut.reply && !REPLY_KINDS.has(item.kind)),
  );
}

/** The menu a bubble's ··· or right-click opens, at the pointer. */
export function MessageMenu({
  at,
  onDelete,
  onClose,
}: {
  at: { x: number; y: number };
  onDelete: () => void;
  onClose: () => void;
}) {
  const ref = useDismiss<HTMLDivElement>(true, onClose);
  const first = useRef<HTMLButtonElement>(null);
  useEffect(() => first.current?.focus(), []);
  return (
    <div ref={ref} className="feed-menu tab-menu" role="menu" aria-label="Message" style={{ left: at.x, top: at.y }}>
      <button
        ref={first}
        type="button"
        role="menuitem"
        onClick={() => {
          onClose();
          onDelete();
        }}
      >
        {DELETE_MESSAGE_LABEL}
      </button>
    </div>
  );
}

/** Asks before Delete message…. Escape or Keep closes it; Delete confirms. */
export function DeleteMessageDialog({
  reply,
  onConfirm,
  onClose,
}: {
  reply: boolean;
  onConfirm: () => void;
  onClose: () => void;
}) {
  const keep = useRef<HTMLButtonElement>(null);
  const question = reply ? DELETE_REPLY_QUESTION : DELETE_ASK_QUESTION;
  useEffect(() => {
    keep.current?.focus();
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);
  return (
    <div className="fix-backdrop" onClick={onClose}>
      <div
        className="card fix-window confirm-window"
        role="alertdialog"
        aria-modal="true"
        aria-label={question}
        onClick={(e) => e.stopPropagation()}
      >
        <h2>{question}</h2>
        <p>{DELETE_WARNING}</p>
        <div className="card-buttons">
          <button type="button" className="danger" onClick={onConfirm}>
            Delete
          </button>
          <button ref={keep} type="button" className="quiet" onClick={onClose}>
            Keep
          </button>
        </div>
      </div>
    </div>
  );
}
