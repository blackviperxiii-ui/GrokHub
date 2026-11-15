import { useEffect, useRef, useState, type FormEvent } from "react";
import { companionNote, companionSetNote } from "./api";
import { useModalFocus } from "./ui";

export const NOTE_HELP = "Shown on the card the next time you launch it.";
export const NOTE_PLACEHOLDER = "remember I was farming X";
export const NOTE_SAVED = "Saved. It shows on the card next time you launch it.";
/** The note's limit, as the companion keeps it (`grokhub_core::game_companion` NOTE_CHARS). */
export const NOTE_CHARS = 500;

/** A game's note for next time, from a "Leave a note" chip. Save keeps it
 * for the launch card; Cancel and Esc save nothing. `onDone` gets the
 * status line after a save, or null. */
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
        field.current?.focus();
      },
      (err) => {
        if (live) setError(String(err));
      },
    );
    return () => {
      live = false;
    };
  }, [appId]);

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
      await companionSetNote(appId, text);
      onDone(NOTE_SAVED);
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
          disabled={game === null && !error}
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
