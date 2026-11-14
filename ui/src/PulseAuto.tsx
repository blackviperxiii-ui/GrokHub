import { useEffect, useState } from "react";
import {
  getPulseAuto,
  setProactiveReminders,
  setPulseAutoFix,
  setPulseAutoPause,
  type PulseAuto as View,
} from "./api";
import { Toggle } from "./Toggle";

export const AUTO_FIXES_HELP =
  "A switched-on kind of fix skips its tap. Each one only reads; anything that changes your computer still asks on its card.";
export const REMINDERS_TITLE = "Reminder notifications";
export const DONE_FOR_YOU_TITLE = "Done for you";
export const REMINDERS_HELP =
  "One notice when a Pulse card is due within the hour. At most three a day, never during a game or quiet hours.";

/**
 * Settings → Pulse: fixes that run on their own, reminder notifications and
 * Done for you. Every switch starts off. Same `app.json` keys as the cabin
 * (`autoFixes`, `proactiveReminders`), plus `pauseFailingAutomations`.
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
      <h2>{DONE_FOR_YOU_TITLE}</h2>
      {view && (
        <>
          <Toggle
            checked={view.pause.on}
            disabled={busy}
            onChange={(next) => void save(() => setPulseAutoPause(next))}
          >
            {view.pause.label}
          </Toggle>
          <p className="note">{view.pause.note}</p>
          {view.pause.on && view.pause.held && (
            <p className="note" role="status">
              {view.pause.held}
            </p>
          )}
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
