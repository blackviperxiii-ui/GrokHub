import { useEffect, useState } from "react";
import { getScreenshotOffer, setScreenshotExplain } from "./screenshotOfferApi";
import { Toggle } from "./Toggle";

/** The cabin's words for Settings → Behavior → Screenshot explain. */
export const SCREENSHOT_EXPLAIN_NOTE =
  "When you save a new screenshot, offer a small Explain chip. The image is not read until you tap.";

/** Settings → Behavior → Screenshot explain, saved where the cabin keeps it. */
export function ScreenshotExplain() {
  const [on, setOn] = useState<boolean | null>(null);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    getScreenshotOffer().then(
      (view) => setOn(view.enabled),
      (err) => setNote(String(err)),
    );
  }, []);

  async function change(next: boolean) {
    try {
      setOn((await setScreenshotExplain(next)).enabled);
      setNote(null);
    } catch (err) {
      setNote(String(err));
    }
  }

  return (
    <div role="group" aria-label="Screenshot explain">
      <h2>Screenshots</h2>
      {on !== null && (
        <Toggle checked={on} onChange={(next) => void change(next)}>
          Screenshot explain
        </Toggle>
      )}
      <p className="note">{SCREENSHOT_EXPLAIN_NOTE}</p>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
    </div>
  );
}
