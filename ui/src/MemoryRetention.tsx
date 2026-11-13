import { useEffect, useState } from "react";
import { getMemoryRetention, setMemoryRetention, type MemoryRetention as Shape } from "./api";

export const RETENTION_NOTE =
  "Unsure memory notes nothing links to are retired after this long, once a day. Retired notes stay on disk, marked forgotten. /memory prune runs it now.";

/** Settings → Behavior → Memory retention. Same app.json key as the cabin. */
export function MemoryRetention() {
  const [row, setRow] = useState<Shape | null>(null);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    getMemoryRetention().then(setRow, (err) => setNote(String(err)));
  }, []);

  async function choose(days: number) {
    try {
      const next = await setMemoryRetention(days);
      setRow(next);
      setNote(`Memory retention: ${next.label}`);
    } catch (err) {
      setNote(String(err));
    }
  }

  return (
    <div className="behavior" role="group" aria-label="Memory retention settings">
      {row && (
        <>
          <label>
            Memory retention
            <select
              aria-label="Memory retention days"
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
          <p className="note">{RETENTION_NOTE}</p>
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
