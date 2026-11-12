// The composer's screenshot offer (the cabin's screenshot_offer_ui): with
// Settings → Behavior → Screenshot explain on, a screenshot saved in the last
// two minutes shows "Explain this?" above the composer. A tap attaches it and
// fills an empty composer; nothing is sent until the person sends.
import { useEffect, useState } from "react";
import type { StageOutcome } from "./api";
import {
  dismissScreenshotOffer,
  explainScreenshotOffer,
  getScreenshotOffer,
  OFFER_ENDED,
  SHOT_GONE,
  type ScreenshotOfferChip,
} from "./screenshotOfferApi";
import { inShell } from "./transport";

/** How often the folders are looked at while Chat is open (the cabin's tick). */
export const SHOT_POLL_MS = 30_000;

export function ScreenshotOffer({
  conversationId,
  active,
  remote,
  busy,
  onAttached,
}: {
  conversationId: string;
  /** The Chat tab shows. */
  active: boolean;
  /** The web app: the screenshots are on the other computer. */
  remote: boolean;
  busy: boolean;
  /** The picture went to the composer's tray (or why it didn't). */
  onAttached: (outcome: StageOutcome, prompt: string) => void;
}) {
  const [offer, setOffer] = useState<ScreenshotOfferChip | null>(null);
  const [working, setWorking] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const watching = active && !remote && inShell();

  useEffect(() => {
    if (!watching) return;
    let gone = false;
    function look() {
      if (document.hidden) return;
      getScreenshotOffer().then(
        (view) => {
          if (gone) return;
          setOffer(view.offer);
          if (!view.offer) setNote(null);
        },
        () => {},
      );
    }
    look();
    const timer = window.setInterval(look, SHOT_POLL_MS);
    // Coming back to the window right after taking a screenshot.
    window.addEventListener("focus", look);
    document.addEventListener("visibilitychange", look);
    return () => {
      gone = true;
      window.clearInterval(timer);
      window.removeEventListener("focus", look);
      document.removeEventListener("visibilitychange", look);
    };
  }, [watching]);

  if (!watching || !offer) return null;
  const shown = offer;

  async function explain() {
    setWorking(true);
    setNote(null);
    try {
      const outcome = await explainScreenshotOffer(conversationId);
      onAttached(outcome, shown.prompt);
      if (outcome.staged) setOffer(null);
      else setNote(outcome.error);
    } catch (err) {
      const why = String(err);
      setNote(why);
      if (why === SHOT_GONE || why === OFFER_ENDED) setOffer(null);
    } finally {
      setWorking(false);
    }
  }

  function notNow() {
    setOffer(null);
    setNote(null);
    void dismissScreenshotOffer().catch(() => {});
  }

  return (
    <div className="attach-chips" role="group" aria-label="New screenshot">
      <button
        type="button"
        className="chip"
        title={shown.name}
        disabled={busy || working}
        onClick={() => void explain()}
      >
        {working ? "Attaching…" : shown.label}
      </button>
      <button type="button" className="chip" disabled={working} onClick={notNow}>
        {shown.dismiss}
      </button>
      {note && (
        <p className="note warn" role="status">
          {note}
        </p>
      )}
    </div>
  );
}
