import { useEffect, useState } from "react";
import { getPcHealth, setPcHealth, type PcHealth as View } from "./api";
import { Toggle } from "./Toggle";

export const PC_HEALTH_TITLE = "PC health while gaming";
export const PC_HEALTH_HELP =
  "While a game is running, sample CPU/GPU temps and util lightly. After you quit, Pulse shows max temps, throttle flags, a GPU vs CPU bound guess, and tips. Never changes fans, clocks, or drivers. Off by default.";

/** Settings → Pulse: the opt-in PC health switch, the cabin's `pcHealth`. */
export function PcHealth() {
  const [view, setView] = useState<View | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getPcHealth().then(setView, (err) => setNote(String(err)));
  }, []);

  async function save(on: boolean) {
    setBusy(true);
    setNote(null);
    try {
      setView(await setPcHealth(on));
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div role="group" aria-label={PC_HEALTH_TITLE}>
      <h2>{PC_HEALTH_TITLE}</h2>
      {view && (
        <Toggle
          checked={view.on}
          disabled={busy}
          onChange={(next) => void save(next)}
        >
          {PC_HEALTH_TITLE}
        </Toggle>
      )}
      <p className="note">{PC_HEALTH_HELP}</p>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
    </div>
  );
}
