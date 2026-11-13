import { useEffect, useState } from "react";
import { gameEso, gameEsoAddon, type EsoView } from "../api";

export function Eso() {
  const [view, setView] = useState<EsoView | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    gameEso().then(
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

  async function act(dir: string, install: boolean) {
    setBusy(true);
    setErr(null);
    try {
      setView(await gameEsoAddon(dir, install));
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
    <div className="game-screen eso-screen" role="region" aria-label="The Elder Scrolls Online">
      {view.empty && <p className="card-status">{view.empty}</p>}
      {view.servers.map((s) => (
        <div key={s.dir} className="game-screen-row">
          <p className="card-status">{s.status}</p>
          {s.action && (
            <button
              type="button"
              disabled={busy}
              onClick={() => void act(s.dir, !s.action!.startsWith("Remove"))}
            >
              {s.action}
            </button>
          )}
        </div>
      ))}
      {view.character && <p className="card-status">{view.character}</p>}
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
