import { useEffect, useState } from "react";
import { getChatArchive, setChatArchive, type ChatArchive as Shape } from "./api";

export const ARCHIVE_NOTE =
  "Chats with no messages, opens or edits for this long leave History and search (still on disk; nothing is deleted). Pinned chats and chats with a running or parked task stay.";

/** Settings → Behavior → Auto-archive idle chats. Same app.json key as the cabin. */
export function ChatArchive() {
  const [row, setRow] = useState<Shape | null>(null);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    getChatArchive().then(setRow, (err) => setNote(String(err)));
  }, []);

  async function choose(days: number) {
    try {
      const next = await setChatArchive(days);
      setRow(next);
      setNote(next.days === 0 ? "Auto-archive off" : `Auto-archive after ${next.days} days`);
    } catch (err) {
      setNote(String(err));
    }
  }

  return (
    <div className="behavior" role="group" aria-label="Auto-archive idle chats settings">
      {row && (
        <>
          <label>
            Auto-archive idle chats
            <select
              aria-label="Auto-archive idle chats days"
              value={row.days}
              onChange={(e) => void choose(Number(e.target.value))}
            >
              {row.choices.map((c) => (
                <option key={c.days} value={c.days}>
                  {c.label}
                </option>
              ))}
            </select>
          </label>
          <p className="note">{ARCHIVE_NOTE}</p>
        </>
      )}
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
    </div>
  );
}
