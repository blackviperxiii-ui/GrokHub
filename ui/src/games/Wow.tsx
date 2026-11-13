import { useEffect, useState } from "react";
import { gameWow, gameWowAddon, type WowView } from "../api";

export function Wow() {
  const [view, setView] = useState<WowView | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    gameWow().then(
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

  async function run(install: string, flavor: string, action: string) {
    setBusy(true);
    setErr(null);
    try {
      setView(await gameWowAddon(install, flavor, action));
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
    <div className="game-screen wow-screen" role="region" aria-label="World of Warcraft">
      <h3>{view.title}</h3>
      {view.running && <p className="game-screen-running">RUNNING</p>}
      <p className="card-status">{view.caption}</p>
      {view.logging && <p className="card-status">{view.logging}</p>}
      {view.watcherNote && <p className="card-status">{view.watcherNote}</p>}
      {view.installAll && view.flavors[0] && (
        <button
          type="button"
          disabled={busy}
          onClick={() => void run(view.flavors[0].install, view.flavors[0].flavor, "install_all")}
        >
          {view.installAll}
        </button>
      )}
      {view.flavors.map((f) => (
        <div key={`${f.install}:${f.flavor}`} className="game-screen-row">
          <p className="card-status">
            {f.label} · {f.addonLine}
          </p>
          {f.action && (
            <button
              type="button"
              disabled={busy}
              onClick={() =>
                void run(
                  f.install,
                  f.flavor,
                  f.action === "Remove addon"
                    ? "remove"
                    : f.action === "Update addon"
                      ? "update"
                      : "install",
                )
              }
            >
              {f.action}
            </button>
          )}
          {f.characters.map((c) => (
            <p key={c} className="card-status">
              {c}
            </p>
          ))}
          {f.events.map((e) => (
            <p key={e} className="card-status">
              {e}
            </p>
          ))}
          {f.emptySaved && <p className="card-status">{f.emptySaved}</p>}
        </div>
      ))}
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
