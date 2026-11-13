import { useEffect, useState } from "react";
import { getWorkboardTidy, setWorkboardTidy, type BoardTidy } from "./api";

export const TIDY_NOTE =
  "A card with no activity for 30 days is archived, and an agent's finished card you never opened goes after a day. The cabin and this app share the setting.";
export const AUTO_DELETE_CHOICES = [
  { days: 0, label: "Off" },
  { days: 30, label: "After 30 days" },
  { days: 90, label: "After 90 days" },
] as const;

/** Settings → Behavior → Workboard: auto-archive is fixed at 30 days; deleting
 * archived cards on their own is off unless you pick 30 or 90 days. */
export function WorkboardTidy() {
  const [tidy, setTidy] = useState<BoardTidy | null>(null);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    getWorkboardTidy().then(setTidy, (err) => setNote(String(err)));
  }, []);

  async function choose(days: number) {
    try {
      setTidy(await setWorkboardTidy(days));
      setNote("Saved.");
    } catch (err) {
      setNote(String(err));
    }
  }

  return (
    <div className="behavior" role="group" aria-label="Workboard tidy">
      <h2>Workboard</h2>
      {tidy && (
        <>
          <p>Auto-archive after {tidy.autoArchiveDays} days</p>
          <label>
            Auto-delete archived
            <select value={tidy.autoDeleteDays} onChange={(e) => void choose(Number(e.target.value))}>
              {AUTO_DELETE_CHOICES.map((c) => (
                <option key={c.days} value={c.days}>
                  {c.label}
                </option>
              ))}
            </select>
          </label>
          <p className="note">{TIDY_NOTE}</p>
        </>
      )}
      {note && <p className="note">{note}</p>}
    </div>
  );
}
