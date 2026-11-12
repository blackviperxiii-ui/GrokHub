import { useEffect, useState } from "react";
import {
  gameDestiny,
  gameDestinyAnswerKey,
  gameDestinyRefresh,
  gameDestinyRemove,
  gameDestinySaveName,
  gameDestinyStartKey,
  type DestinyView,
} from "../api";

export function Destiny() {
  const [view, setView] = useState<DestinyView | null>(null);
  const [keyDraft, setKeyDraft] = useState("");
  const [nameDraft, setNameDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [cardId, setCardId] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    gameDestiny().then(
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

  async function wrap(fn: () => Promise<DestinyView>) {
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
      const card = await gameDestinyStartKey(keyDraft);
      setCardId(card.id);
      setKeyDraft("");
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

  const ready = view.hasKey && view.hasName;

  return (
    <div className="game-screen destiny-screen" role="region" aria-label="Destiny 2">
      <p className="card-status">{view.status}</p>
      {ready ? (
        <div className="game-screen-actions">
          <button type="button" disabled={busy} onClick={() => void wrap(() => gameDestinyRefresh())}>
            {busy ? "Refreshing..." : "Refresh Destiny 2"}
          </button>
          <button type="button" disabled={busy} onClick={() => void wrap(() => gameDestinyRemove())}>
            Remove key and name
          </button>
        </div>
      ) : (
        <>
          <p className="card-status">{view.howTo}</p>
          {!view.hasKey && (
            <>
              <label className="game-screen-field">
                <span>Bungie.net API key</span>
                <input
                  type="password"
                  autoComplete="off"
                  value={keyDraft}
                  onChange={(e) => setKeyDraft(e.target.value)}
                  aria-label="Bungie.net API key"
                />
              </label>
              <p className="card-status">{view.keyHint}</p>
              <button type="button" disabled={busy || !keyDraft.trim()} onClick={() => void saveKey()}>
                Save key
              </button>
            </>
          )}
          {!view.hasName && (
            <>
              <label className="game-screen-field">
                <span>Bungie Name</span>
                <input
                  type="text"
                  autoComplete="off"
                  value={nameDraft}
                  onChange={(e) => setNameDraft(e.target.value)}
                  aria-label="Bungie Name"
                />
              </label>
              <p className="card-status">{view.nameHint}</p>
              <button
                type="button"
                disabled={busy || !nameDraft.trim()}
                onClick={() => void wrap(() => gameDestinySaveName(nameDraft))}
              >
                Save name
              </button>
            </>
          )}
        </>
      )}
      {cardId && (
        <div className="game-screen-actions" role="group" aria-label="Save the Bungie.net API key">
          <p className="card-status">Save the Bungie.net API key?</p>
          <button
            type="button"
            disabled={busy}
            onClick={() =>
              void wrap(async () => {
                const v = await gameDestinyAnswerKey(cardId, true);
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
                const v = await gameDestinyAnswerKey(cardId, false);
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
