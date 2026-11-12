import { useEffect, useState } from "react";
import { gameElite, type EliteView } from "../api";

export function Elite() {
  const [view, setView] = useState<EliteView | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    gameElite().then(
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

  if (err && !view) {
    return <p className="card-status">{err}</p>;
  }
  if (!view) {
    return <p className="card-status">Loading…</p>;
  }

  return (
    <div className="game-screen elite-screen" role="region" aria-label="Elite Dangerous">
      <p className="game-screen-title">{view.title}</p>
      {view.status && <p className="card-status">{view.status}</p>}
      {view.empty && <p className="card-status">{view.empty}</p>}
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
