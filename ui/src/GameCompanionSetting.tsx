import { useEffect, useState } from "react";
import { getGameCompanion, setGameCompanion, setGameCompanionPing } from "./api";
import { Toggle } from "./Toggle";

export const COMPANION_TITLE = "Game launch companion";
export const COMPANION_HELP =
  "When you start a Steam game, Pulse gets one card: what changed since you last played, a link to top guides, and your note from last time. When the game closes, a chip offers a note for next time. Nothing opens or touches the game.";
export const PING_TITLE = "Also send a quiet desktop notification";
export const PING_OFF_NOTE = "Notifications are off in Settings → General, so this stays quiet.";

/** Settings → Behavior → Game launch companion, with its optional ping. The
 * ping toggle shows only while the companion is on. */
export function GameCompanionSetting({ notifications }: { notifications: boolean }) {
  const [on, setOn] = useState<boolean | null>(null);
  const [ping, setPing] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getGameCompanion().then(
      (view) => {
        setOn(view.on);
        setPing(view.ping);
      },
      (err) => setNote(String(err)),
    );
  }, []);

  async function change(work: () => ReturnType<typeof setGameCompanion>) {
    setBusy(true);
    try {
      const view = await work();
      setOn(view.on);
      setPing(view.ping);
      setNote(view.note || null);
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div role="group" aria-label={COMPANION_TITLE}>
      <h2>{COMPANION_TITLE}</h2>
      {on !== null && (
        <Toggle
          checked={on}
          disabled={busy}
          onChange={(next) => void change(() => setGameCompanion(next))}
        >
          {COMPANION_TITLE}
        </Toggle>
      )}
      <p className="note">{COMPANION_HELP}</p>
      {on && (
        <div className="companion-ping">
          <Toggle
            checked={ping}
            disabled={busy}
            onChange={(next) => void change(() => setGameCompanionPing(next))}
          >
            {PING_TITLE}
          </Toggle>
          {ping && !notifications && <p className="note warn">{PING_OFF_NOTE}</p>}
        </div>
      )}
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
    </div>
  );
}
