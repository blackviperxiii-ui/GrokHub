import { useEffect, useState } from "react";
import { gameFactorio, gameFactorioMod, type FactorioView } from "../api";

export function Factorio() {
  const [view, setView] = useState<FactorioView | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    gameFactorio().then(
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

  async function act(install: boolean) {
    if (!view?.userDir) return;
    setBusy(true);
    setErr(null);
    try {
      setView(await gameFactorioMod(view.userDir, install));
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
    <div className="game-screen factorio-screen" role="region" aria-label="Factorio">
      {view.empty && <p className="card-status">{view.empty}</p>}
      {view.status && <p className="card-status">{view.status}</p>}
      {view.warning && <p className="card-status">{view.warning}</p>}
      {view.action && (
        <button
          type="button"
          disabled={busy}
          onClick={() => void act(!view.action!.startsWith("Remove"))}
        >
          {view.action}
        </button>
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
