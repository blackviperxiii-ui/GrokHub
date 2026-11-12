import { useEffect, useState, type MouseEvent } from "react";
import {
  changeRowKeep,
  changeRowRestore,
  changeRowUndo,
  getChangeRows,
  type ChangeAnswer,
  type ChangeRow,
  type ChangeRowsView,
} from "./changeRowsApi";
import { ConfirmRow } from "./ui";

const ROWS = { listStyle: "none", margin: 0, padding: 0, display: "grid", gap: 8 } as const;

/** Undo, Keep and Restore answer a pointer click only: Enter or Space on a focused pill does nothing. */
function pointer(e: MouseEvent) {
  return e.detail > 0;
}

/** Settings → Behavior: what the app changed on its own in skills,
 * connections and automations, with Undo, Keep and Restore, from the same
 * change ledger as `/skills changes`. Undo and Restore ask first. */
export function ChangeRows() {
  const [view, setView] = useState<ChangeRowsView | null>(null);
  const [asking, setAsking] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getChangeRows().then(setView, (err) => setStatus(String(err)));
  }, []);

  async function run(work: () => Promise<ChangeAnswer>) {
    setBusy(true);
    setAsking(null);
    try {
      const got = await work();
      setView(got.view);
      setStatus(got.status);
    } catch (err) {
      setStatus(String(err));
    } finally {
      setBusy(false);
    }
  }

  function keep(e: MouseEvent, kind: string, row: ChangeRow) {
    if (!pointer(e) || busy) return;
    void run(() => changeRowKeep(kind, row.id));
  }

  function ask(e: MouseEvent, key: string) {
    if (!pointer(e) || busy) return;
    setAsking(key);
  }

  if (!view) return status ? <p className="note">{status}</p> : null;

  return (
    <section aria-label={view.heading}>
      <h2>{view.heading}</h2>
      {view.groups.length === 0 && <p className="note">{view.empty}</p>}
      {view.groups.map((g) => (
        <ul key={g.kind} style={ROWS} aria-label={g.title}>
          <li>
            <strong>{g.title}</strong>
          </li>
          {g.rows.map((r) => {
            const key = `${g.kind}:${r.id}`;
            const verb = r.act === "undo" ? "Undo" : "Restore";
            return (
              <li key={key} aria-label={r.label}>
                <p className="card-status">{r.label}</p>
                {asking === key ? (
                  <ConfirmRow
                    question={`${verb} ${r.id}?`}
                    detail={
                      r.act === "undo" ? "The version before it comes back." : "It comes back from its kept copy."
                    }
                    action={verb}
                    busy={busy}
                    onConfirm={() =>
                      void run(() => (r.act === "undo" ? changeRowUndo(g.kind, r.id) : changeRowRestore(g.kind, r.id)))
                    }
                    onCancel={() => setAsking(null)}
                  />
                ) : (
                  <div className="card-buttons">
                    <button type="button" className="quiet" disabled={busy} onClick={(e) => ask(e, key)}>
                      {verb}
                    </button>
                    {r.keep && (
                      <button type="button" className="quiet" disabled={busy} onClick={(e) => keep(e, g.kind, r)}>
                        Keep
                      </button>
                    )}
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      ))}
      {status && (
        <p className="note" role="status">
          {status}
        </p>
      )}
    </section>
  );
}
