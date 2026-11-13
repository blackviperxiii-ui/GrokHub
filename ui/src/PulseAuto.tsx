import { useEffect, useState } from "react";
import {
  getPulseAuto,
  setProactiveReminders,
  setPulseAutoFix,
  type PulseAuto as View,
} from "./api";
import { Toggle } from "./Toggle";

export const AUTO_FIXES_HELP =
  "A switched-on kind of fix skips its tap. Each one only reads; anything that changes your computer still asks on its card.";
export const REMINDERS_TITLE = "Reminder notifications";
export const REMINDERS_HELP =
  "One notice when a Pulse card is due within the hour. At most three a day, never during a game or quiet hours.";

/**
 * Settings → Pulse: fixes that run on their own and reminder notifications.
 * Every switch starts off. Same `app.json` keys as the cabin (`autoFixes`,
 * `proactiveReminders`).
 */
export function PulseAuto() {
  const [view, setView] = useState<View | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getPulseAuto().then(setView, (err) => setNote(String(err)));
  }, []);

  async function save(change: () => Promise<View>) {
    setBusy(true);
    setNote(null);
    try {
      setView(await change());
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div role="group" aria-label="Fixes and reminders">
      <h2>Fixes that run on their own</h2>
      <p className="note">{AUTO_FIXES_HELP}</p>
      {view?.fixes.map((fix) => (
        <div key={fix.id}>
          <Toggle
            checked={fix.on}
            disabled={busy}
            onChange={(next) => void save(() => setPulseAutoFix(fix.id, next))}
          >
            {fix.label}
          </Toggle>
          <p className="note">{fix.note}</p>
        </div>
      ))}
      <h2>{REMINDERS_TITLE}</h2>
      {view && (
        <Toggle
          checked={view.reminders}
          disabled={busy}
          onChange={(next) => void save(() => setProactiveReminders(next))}
        >
          {REMINDERS_TITLE}
        </Toggle>
      )}
      <p className="note">{REMINDERS_HELP}</p>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
    </div>
  );
}
