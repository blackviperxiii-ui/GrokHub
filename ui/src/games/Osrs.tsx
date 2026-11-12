import { useEffect, useState } from "react";
import {
  gameOsrs,
  gameOsrsForget,
  gameOsrsRefresh,
  gameOsrsSave,
  type OsrsView,
} from "../api";

export function Osrs() {
  const [view, setView] = useState<OsrsView | null>(null);
  const [name, setName] = useState("");
  const [kind, setKind] = useState("Normal");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    gameOsrs().then(
      (v) => {
        if (!live) return;
        setView(v);
        setKind(v.kind);
      },
      (e) => {
        if (live) setErr(String(e));
      },
    );
    return () => {
      live = false;
    };
  }, []);

  async function wrap(fn: () => Promise<OsrsView>) {
    setBusy(true);
    setErr(null);
    try {
      const v = await fn();
      setView(v);
      setKind(v.kind);
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
    <div className="game-screen osrs-screen" role="region" aria-label="Old School RuneScape">
      <p className="card-status">{view.status}</p>
      {view.gains && <p className="card-status">{view.gains}</p>}
      {view.hasPlayer ? (
        <div className="game-screen-actions">
          <button type="button" disabled={busy} onClick={() => void wrap(() => gameOsrsRefresh())}>
            {busy ? "Refreshing..." : "Refresh hiscores"}
          </button>
          <button type="button" disabled={busy} onClick={() => void wrap(() => gameOsrsForget())}>
            Forget player
          </button>
        </div>
      ) : (
        <>
          <label className="game-screen-field">
            <span>Old School RuneScape name</span>
            <input
              type="text"
              value={name}
              onChange={(e) => setName(e.target.value)}
              aria-label="Old School RuneScape name"
            />
          </label>
          <p className="card-status">{view.nameHint}</p>
          <label className="game-screen-field">
            <span>Account type</span>
            <select
              aria-label="Account type"
              value={kind}
              onChange={(e) => setKind(e.target.value)}
            >
              {view.kinds.map((k) => (
                <option key={k} value={k}>
                  {k}
                </option>
              ))}
            </select>
          </label>
          <button
            type="button"
            disabled={busy || !name.trim()}
            onClick={() => void wrap(() => gameOsrsSave(name, kind))}
          >
            Save name
          </button>
        </>
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
