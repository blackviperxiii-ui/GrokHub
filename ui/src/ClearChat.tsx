// The Chat tab's right-click menu and its Clear chat confirm. Clearing deletes
// the open chat's saved history for good, so the confirm says so plainly.
import { useEffect, useRef } from "react";
import { PRODUCT_NAME } from "./brand";
import { useDismiss } from "./ui";

export const CLEAR_CHAT_LABEL = "Clear chat…";
export const CLEAR_CHAT_QUESTION = "Clear this chat?";
export const CLEAR_CHAT_WARNING = `This is permanent. Every message in this chat and the history ${PRODUCT_NAME} reads back are deleted from this computer, and they can't be brought back.`;
export const CLEAR_CHAT_KEEPS = "Your other chats, the Workboard, memory and settings stay as they are.";

/** The menu a right-click on the Chat tab opens, at the pointer. */
export function ChatTabMenu({ at, onClear, onClose }: { at: { x: number; y: number }; onClear: () => void; onClose: () => void }) {
  const ref = useDismiss<HTMLDivElement>(true, onClose);
  const first = useRef<HTMLButtonElement>(null);
  useEffect(() => first.current?.focus(), []);
  return (
    <div ref={ref} className="feed-menu tab-menu" role="menu" aria-label="Chat" style={{ left: at.x, top: at.y }}>
      <button
        ref={first}
        type="button"
        role="menuitem"
        onClick={() => {
          onClose();
          onClear();
        }}
      >
        {CLEAR_CHAT_LABEL}
      </button>
    </div>
  );
}

/** Asks before Clear chat. Escape or Keep closes it; Clear chat confirms. */
export function ClearChatDialog({ onConfirm, onClose }: { onConfirm: () => void; onClose: () => void }) {
  const keep = useRef<HTMLButtonElement>(null);
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
        aria-label={CLEAR_CHAT_QUESTION}
        onClick={(e) => e.stopPropagation()}
      >
        <h2>{CLEAR_CHAT_QUESTION}</h2>
        <p>{CLEAR_CHAT_WARNING}</p>
        <p className="confirm-keeps">{CLEAR_CHAT_KEEPS}</p>
        <div className="card-buttons">
          <button type="button" className="danger" onClick={onConfirm}>
            Clear chat
          </button>
          <button ref={keep} type="button" className="quiet" onClick={onClose}>
            Keep
          </button>
        </div>
      </div>
    </div>
  );
}
