import { useEffect, useState, type FormEvent } from "react";
import {
  forgetSteamId,
  getSteamPulse,
  saveSteamId,
  setSteamAchievements,
  setSteamWishlist,
  setSteamWishlistThreshold,
  steamCheckNow,
  type SteamPulse,
} from "./api";
import { Toggle } from "./Toggle";

export const WISHLIST_LABEL = "Alert me when wishlisted games go on sale";
export const WISHLIST_HELP =
  "Once a day, from your public Steam wishlist and Steam's store prices. Your SteamID is sealed on this PC and only sent to Steam.";
export const ACHIEVEMENTS_LABEL = "Nudge me when a game is close to 100% achievements";
export const ACHIEVEMENTS_HELP =
  "Once a week, from your public Steam achievements for the games you play most, with links to Steam's free guides. Uses the SteamID saved for wishlist alerts.";
export const STEAM_ID_SAVED = "SteamID saved";
export const WISH_PRIVATE =
  "Your Steam wishlist looks private or empty. In Steam, set Privacy → Game details to Public to get alerts.";
export const ACH_PRIVATE =
  "Your Steam game details look private. In Steam, set Privacy → Game details to Public to get nudges.";

/**
 * Settings → Pulse: Steam wishlist sale alerts and achievement nudges.
 * SteamID is sealed; the UI never shows the digits.
 */
export function SteamPulseSources() {
  const [page, setPage] = useState<SteamPulse | null>(null);
  const [draft, setDraft] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getSteamPulse().then(setPage, (err) => setNote(String(err)));
  }, []);

  async function run(work: () => Promise<SteamPulse>) {
    setBusy(true);
    try {
      const next = await work();
      setPage(next);
      setNote(null);
      return next;
    } catch (err) {
      setNote(String(err));
      return null;
    } finally {
      setBusy(false);
    }
  }

  async function saveId(event: FormEvent) {
    event.preventDefault();
    if (busy || !draft.trim()) return;
    if (await run(() => saveSteamId(draft, false))) setDraft("");
  }

  function idEntry() {
    return (
      <div className="steam-id-entry">
        <form className="labs-row" onSubmit={(e) => void saveId(e)}>
          <label>
            SteamID64
            <input
              type="password"
              autoComplete="off"
              value={draft}
              maxLength={80}
              placeholder="SteamID64 or /profiles/ link"
              onChange={(e) => setDraft(e.target.value)}
              aria-label="SteamID64 or profiles link"
            />
          </label>
          <button type="submit" disabled={busy || !draft.trim()}>
            Save
          </button>
        </form>
        {page?.canUseLocal && (
          <button
            type="button"
            disabled={busy}
            onClick={() => void run(() => saveSteamId("", true))}
          >
            Use the account on this PC
          </button>
        )}
      </div>
    );
  }

  function idSaved(module: "achievements" | "deals") {
    return (
      <div className="labs-row">
        <span className="note">{STEAM_ID_SAVED}</span>
        <button
          type="button"
          disabled={busy}
          onClick={() => void run(() => steamCheckNow(module))}
        >
          Check now
        </button>
        <button type="button" disabled={busy} onClick={() => void run(() => forgetSteamId())}>
          Forget
        </button>
      </div>
    );
  }

  return (
    <div className="steam-pulse-sources" role="group" aria-label="Steam Pulse sources">
      <h2>Steam sources</h2>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      {page === null && !note && <p className="empty">Loading Steam sources…</p>}
      {page && (
        <>
          <div role="group" aria-label="Wishlist alerts">
            <Toggle
              checked={page.wishlistOn}
              disabled={busy}
              onChange={(next) => void run(() => setSteamWishlist(next))}
            >
              {WISHLIST_LABEL}
            </Toggle>
            {page.wishlistOn && (
              <div className="steam-source-detail">
                <p className="note">{WISHLIST_HELP}</p>
                {page.hasId ? idSaved("deals") : idEntry()}
                <label>
                  Alert on
                  <select
                    aria-label="Wishlist sale threshold"
                    value={page.threshold}
                    disabled={busy}
                    onChange={(e) => void run(() => setSteamWishlistThreshold(e.target.value))}
                  >
                    {page.thresholds.map((t) => (
                      <option key={t.key} value={t.key}>
                        {t.label}
                      </option>
                    ))}
                  </select>
                </label>
                {page.wishPrivate && <p className="note">{WISH_PRIVATE}</p>}
                {page.wishNote && <p className="note">{page.wishNote}</p>}
              </div>
            )}
          </div>
          <div role="group" aria-label="Achievement nudges">
            <Toggle
              checked={page.achievementsOn}
              disabled={busy}
              onChange={(next) => void run(() => setSteamAchievements(next))}
            >
              {ACHIEVEMENTS_LABEL}
            </Toggle>
            {page.achievementsOn && (
              <div className="steam-source-detail">
                <p className="note">{ACHIEVEMENTS_HELP}</p>
                {page.hasId ? idSaved("achievements") : idEntry()}
                {page.achPrivate && <p className="note">{ACH_PRIVATE}</p>}
                {page.achNote && <p className="note">{page.achNote}</p>}
              </div>
            )}
          </div>
        </>
      )}
    </div>
  );
}
