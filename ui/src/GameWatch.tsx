import { useEffect, useState } from "react";
import { getGameWatch, setGameWatch } from "./api";
import { Toggle } from "./Toggle";

/** The cabin's words for Settings → Behavior → Game watcher. */
export const GAME_WATCH_TITLE = "Game watcher (Counter-Strike 2, Dota 2)";
export const GAME_WATCH_HELP =
  "Follows your match as you play, through Game State Integration, the feed Valve built and supports for this. Turning it on writes one small file into the game's own config folder that tells the game to send its state to Loadout, here on this computer only, while the game runs. Turning it off removes that file. Nothing reads the game's memory or touches the game.";

/**
 * Game watcher switch. Full row in Settings → Behavior; compact status in the
 * Library companion frame. Same `app.json` `gameWatch` key as the cabin.
 */
export function GameWatch({ compact = false }: { compact?: boolean }) {
  const [on, setOn] = useState<boolean | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getGameWatch().then(
      (view) => {
        setOn(view.on);
        setNote(view.note || null);
      },
      (err) => setNote(String(err)),
    );
  }, []);

  async function change(next: boolean) {
    setBusy(true);
    try {
      const view = await setGameWatch(next);
      setOn(view.on);
      setNote(view.note || null);
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  if (compact) {
    return (
      <div className="game-watch-compact" role="group" aria-label="Game watcher">
        {on !== null && (
          <Toggle checked={on} disabled={busy} onChange={(next) => void change(next)}>
            {GAME_WATCH_TITLE}
          </Toggle>
        )}
        {note && (
          <p className="note" role="status">
            {note}
          </p>
        )}
      </div>
    );
  }

  return (
    <div role="group" aria-label="Game watcher">
      <h2>Game watcher</h2>
      {on !== null && (
        <Toggle checked={on} disabled={busy} onChange={(next) => void change(next)}>
          {GAME_WATCH_TITLE}
        </Toggle>
      )}
      <p className="note">{GAME_WATCH_HELP}</p>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
    </div>
  );
}
