// The find bar over the chat (Ctrl+F): type to find, Enter for the next
// match, Shift+Enter for the one before, Esc to close.
import type { KeyboardEvent } from "react";

export const FIND_LABEL = "Find in chat";
export const FIND_PLACEHOLDER = "Find in this chat";

export function ChatFindBar({
  query,
  label,
  hasHits,
  onQuery,
  onStep,
  onClose,
}: {
  query: string;
  /** "2 of 5", "No matches", or "" while the box is blank. */
  label: string;
  hasHits: boolean;
  onQuery: (query: string) => void;
  onStep: (forward: boolean) => void;
  onClose: () => void;
}) {
  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Enter") {
      event.preventDefault();
      onStep(!event.shiftKey);
    } else if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onClose();
    }
  }
  return (
    <div className="chat-find" role="search" aria-label={FIND_LABEL}>
      <input
        type="text"
        value={query}
        placeholder={FIND_PLACEHOLDER}
        aria-label={FIND_PLACEHOLDER}
        // The bar opens from a key press; typing goes straight into it.
        autoFocus
        onChange={(event) => onQuery(event.target.value)}
        onKeyDown={onKeyDown}
      />
      <span className="chat-find-count" aria-live="polite">
        {label}
      </span>
      <button type="button" className="quiet" disabled={!hasHits} title="Previous (Shift+Enter)" aria-label="Previous match" onClick={() => onStep(false)}>
        ↑
      </button>
      <button type="button" className="quiet" disabled={!hasHits} title="Next (Enter)" aria-label="Next match" onClick={() => onStep(true)}>
        ↓
      </button>
      <button type="button" className="quiet" title="Close (Esc)" aria-label="Close find" onClick={onClose}>
        ×
      </button>
    </div>
  );
}
