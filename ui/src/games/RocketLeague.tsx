import { useEffect, useState } from "react";
import { gameRl, type RocketLeagueView } from "../api";

export function RocketLeague() {
  const [view, setView] = useState<RocketLeagueView | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    let live = true;
    gameRl().then(
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

  async function copySetting() {
    if (!view) return;
    try {
      await navigator.clipboard.writeText(view.iniLine);
      setCopied(true);
    } catch (e) {
      setErr(String(e));
    }
  }

  if (err && !view) {
    return <p className="card-status">{err}</p>;
  }
  if (!view) {
    return <p className="card-status">Loading…</p>;
  }

  return (
    <div className="game-screen rl-screen" role="region" aria-label="Rocket League">
      <p className="game-screen-title">{view.title}</p>
      <p className="card-status">{view.status}</p>
      {view.howTo && (
        <button type="button" onClick={() => void copySetting()}>
          {copied ? "Copied" : "Copy the setting"}
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
