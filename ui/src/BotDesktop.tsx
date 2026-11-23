import { useEffect, useState } from "react";
import {
  botDesktopFrame,
  getBotDesktop,
  PREVIEW_EVERY_MS,
  setBotDesktop,
  type BotDesktop as Shape,
} from "./botDesktopApi";
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

  const running = Boolean(shown?.on && shown.display);
  const frame = usePreview(running);

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
      {running && <p className="note">Running on display {shown.display}.</p>}
      {running && frame && (
        <img className="bot-desktop-preview" src={frame} alt="What the Bot Desktop shows now" />
      )}
      {problem && <p className="note warn">{problem}</p>}
    </div>
  );
}

/** A new frame every [`PREVIEW_EVERY_MS`] while `running` and the window is shown. */
function usePreview(running: boolean): string | null {
  const [frame, setFrame] = useState<string | null>(null);
  useEffect(() => {
    if (!running) return;
    let live = true;
    const grab = () => {
      if (document.hidden) return;
      botDesktopFrame().then(
        (next) => {
          if (live) setFrame(next);
        },
        () => {},
      );
    };
    grab();
    const timer = window.setInterval(grab, PREVIEW_EVERY_MS);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
  }, [running]);
  // A stopped screen shows no old frame.
  return running ? frame : null;
}
