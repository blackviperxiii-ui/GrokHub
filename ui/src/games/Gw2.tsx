import { useEffect, useState } from "react";
import {
  gameGw2,
  gameGw2AnswerKey,
  gameGw2Refresh,
  gameGw2RemoveKey,
  gameGw2StartKey,
  type Gw2View,
} from "../api";

export function Gw2() {
  const [view, setView] = useState<Gw2View | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [cardId, setCardId] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    gameGw2().then(
      (v) => {
        if (live) setView(v);
      },
      (e) => {
        if (live) setErr(String(e));
      },
    );
    return () => {
      live = false;
    };
  }, []);

  async function wrap(fn: () => Promise<Gw2View>) {
    setBusy(true);
    setErr(null);
    try {
      setView(await fn());
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function saveKey() {
    setBusy(true);
    setErr(null);
    try {
      const card = await gameGw2StartKey(draft);
      setCardId(card.id);
      setDraft("");
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  if (err && !view) {
    return <p className="card-status">{err}</p>;
  }
  if (!view) {
    return <p className="card-status">Loading…</p>;
  }

  return (
    <div className="game-screen gw2-screen" role="region" aria-label="Guild Wars 2">
      <p className="card-status">{view.status}</p>
      {view.hasKey ? (
        <div className="game-screen-actions">
          <button type="button" disabled={busy} onClick={() => void wrap(() => gameGw2Refresh())}>
            {busy ? "Refreshing..." : "Refresh Guild Wars 2"}
          </button>
          <button type="button" disabled={busy} onClick={() => void wrap(() => gameGw2RemoveKey())}>
            Remove key
          </button>
        </div>
      ) : (
        <>
          <label className="game-screen-field">
            <span>Guild Wars 2 API key</span>
            <input
              type="password"
              autoComplete="off"
              value={draft}
              onChange={(e) => setDraft(e.target.value)}
              aria-label="Guild Wars 2 API key"
            />
          </label>
          <p className="card-status">{view.keyHint}</p>
          <button type="button" disabled={busy || !draft.trim()} onClick={() => void saveKey()}>
            Save key
          </button>
        </>
      )}
      {cardId && (
        <div className="game-screen-actions" role="group" aria-label="Save the Guild Wars 2 API key">
          <p className="card-status">Save the Guild Wars 2 API key?</p>
          <button
            type="button"
            disabled={busy}
            onClick={() =>
              void wrap(async () => {
                const v = await gameGw2AnswerKey(cardId, true);
                setCardId(null);
                return v;
              })
            }
          >
            Save
          </button>
          <button
            type="button"
            disabled={busy}
            onClick={() =>
              void wrap(async () => {
                const v = await gameGw2AnswerKey(cardId, false);
                setCardId(null);
                return v;
              })
            }
          >
            Deny
          </button>
        </div>
      )}
      {view.note && (
        <p className="note" role="status">
          {view.note}
        </p>
      )}
      {err && (
        <p className="note" role="status">
          {err}
        </p>
      )}
    </div>
  );
}
