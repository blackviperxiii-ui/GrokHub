import { useCallback, useEffect, useLayoutEffect, useRef, useState, type FormEvent } from "react";
import { companionNote, companionSetNote } from "./api";
import { useModalFocus } from "./ui";

export const NOTE_HELP = "Shown on the card the next time you launch it.";
export const NOTE_PLACEHOLDER = "remember I was farming X";
export const NOTE_SAVED = "Saved. It shows on the card next time you launch it.";
export const NOTE_REMOVED = "Removed. The next launch card has no note.";
/** How long the status line a save leaves stays under the row. */
export const NOTE_STATUS_MS = 6_000;
/** The note's limit, as the companion keeps it (`grokhub_core::game_companion` NOTE_CHARS). */
export const NOTE_CHARS = 500;

/** The status line a note dialog leaves under the row; it goes on its own. */
export function useNoteStatus(): [string | null, (status: string | null) => void] {
  // Each set counts, so a second save with the same words starts its own
  // timer instead of clearing early on the first one's.
  const [shown, setShown] = useState<{ text: string | null; n: number }>({ text: null, n: 0 });
  useEffect(() => {
    if (!shown.text) return;
    const timer = window.setTimeout(() => setShown((s) => ({ text: null, n: s.n })), NOTE_STATUS_MS);
    return () => window.clearTimeout(timer);
  }, [shown]);
  const set = useCallback((text: string | null) => setShown((s) => ({ text, n: s.n + 1 })), []);
  return [shown.text, set];
}

/** A game's note for next time, from a "Leave a note" chip. Save keeps it
 * for the launch card, and saving an empty box removes it; Cancel and Esc
 * save nothing. `onDone` gets the status line after a save, or null. */
export function GameNote({
  appId,
  onDone,
}: {
  appId: number;
  onDone: (status: string | null) => void;
}) {
  const [game, setGame] = useState<string | null>(null);
  const [text, setText] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const box = useRef<HTMLFormElement>(null);
  const field = useRef<HTMLTextAreaElement>(null);
  useModalFocus(box);

  useEffect(() => {
    let live = true;
    companionNote(appId).then(
      (found) => {
        if (!live) return;
        setGame(found.game);
        setText(found.note);
      },
      (err) => {
        if (live) setError(String(err));
      },
    );
    return () => {
      live = false;
    };
  }, [appId]);

  // The box turns on once the game is known; focus goes there in the same
  // commit that shows the note, so nothing (a keypress, a test) sees the note
  // without the focus.
  useLayoutEffect(() => {
    if (game !== null) field.current?.focus();
  }, [game]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      const owner = (e.target as HTMLElement | null)?.closest?.("[role='dialog']");
      if (owner && owner !== box.current) return;
      onDone(null);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onDone]);

  async function save(event: FormEvent) {
    event.preventDefault();
    if (busy) return;
    setBusy(true);
    try {
      const kept = await companionSetNote(appId, text);
      onDone(kept ? NOTE_SAVED : NOTE_REMOVED);
    } catch (err) {
      setError(String(err));
      setBusy(false);
    }
  }

  const title = `Note for ${game ?? "this game"}`;
  return (
    <div className="fix-backdrop" onClick={() => onDone(null)}>
      <form
        ref={box}
        className="card fix-window game-note"
        role="dialog"
        aria-modal="true"
        aria-label={title}
        onClick={(e) => e.stopPropagation()}
        onSubmit={(e) => void save(e)}
      >
        <h2>{title}</h2>
        <p className="note">{NOTE_HELP}</p>
        <textarea
          ref={field}
          aria-label="Your note"
          rows={3}
          maxLength={NOTE_CHARS}
          placeholder={NOTE_PLACEHOLDER}
          value={text}
          disabled={game === null}
          onChange={(e) => setText(e.target.value)}
        />
        {error && (
          <p className="note warn" role="alert">
            {error}
          </p>
        )}
        <div className="card-buttons">
          <button type="submit" className="primary" disabled={busy || game === null}>
            Save
          </button>
          <button type="button" className="quiet" onClick={() => onDone(null)}>
            Cancel
          </button>
        </div>
      </form>
    </div>
  );
}
