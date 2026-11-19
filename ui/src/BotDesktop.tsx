import { useEffect, useState } from "react";
import { getBotDesktop, setBotDesktop, type BotDesktop as Shape } from "./botDesktopApi";
import { PRODUCT_NAME } from "./brand";
import { Toggle } from "./Toggle";

export const BOT_DESKTOP_NOTE =
  "The desktop tools work on a hidden screen of their own, so your cursor, keys and windows stay yours. Apps the agent opens run there.";

/** Settings → General → Bot Desktop. Hidden where it can't run. */
export function BotDesktop() {
  const [shown, setShown] = useState<Shape | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getBotDesktop().then(setShown, () => setShown(null));
  }, []);

  if (!shown?.supported) return null;

  async function flip(on: boolean) {
    setBusy(true);
    setError(null);
    try {
      setShown(await setBotDesktop(on));
    } catch (err) {
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  const problem = error ?? shown.problem;
  return (
    <div className="settings-group" role="group" aria-label="Bot Desktop">
      <h2>Bot Desktop</h2>
      <Toggle checked={shown.on} disabled={busy} onChange={(on) => void flip(on)}>
        Let {PRODUCT_NAME} work on its own screen
      </Toggle>
      <p className="note">{BOT_DESKTOP_NOTE}</p>
      {shown.on && shown.display && <p className="note">Running on display {shown.display}.</p>}
      {problem && <p className="note warn">{problem}</p>}
    </div>
  );
}
