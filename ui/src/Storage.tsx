import { useEffect, useState } from "react";
import { getStorage, storageBringOver, storageOpen, storageReset, type StorageAnswer, type StorageView } from "./storageApi";
import { ConfirmRow } from "./ui";

const ROWS = { listStyle: "none", margin: 0, padding: 0, display: "grid", gap: 6 } as const;
const ROW = { display: "flex", alignItems: "center", gap: 8, justifyContent: "space-between" } as const;

/** Settings → Privacy & data → Storage: one Open per folder, where local
 * models sit, Reset to default, and Bring them over from the folders older
 * versions used. Reset and Bring them over ask first. */
export function Storage() {
  const [view, setView] = useState<StorageView | null>(null);
  const [asking, setAsking] = useState<"reset" | "bring" | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getStorage().then(setView, (err) => setStatus(String(err)));
  }, []);

  async function run(work: () => Promise<StorageAnswer>) {
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

  async function open(key: string) {
    try {
      await storageOpen(key);
    } catch (err) {
      setStatus(String(err));
    }
  }

  if (!view) return status ? <p className="note">{status}</p> : null;

  return (
    <section aria-label="Storage">
      <h2>Storage</h2>
      <p className="note">{view.filesLine}</p>
      <ul style={ROWS} aria-label="Folders">
        {[...view.folders, view.models].map((f) => (
          <li key={f.key} style={ROW} aria-label={f.line}>
            <span className="card-status">{f.line}</span>
            <button type="button" className="quiet" onClick={() => void open(f.key)}>
              Open
            </button>
          </li>
        ))}
      </ul>
      {view.canReset &&
        (asking === "reset" ? (
          <ConfirmRow
            question="Reset to the default folder?"
            detail="New files go there again. Files already saved stay where they are."
            action="Reset"
            busy={busy}
            onConfirm={() => void run(storageReset)}
            onCancel={() => setAsking(null)}
          />
        ) : (
          <div className="card-buttons">
            <button type="button" className="quiet" disabled={busy} onClick={() => setAsking("reset")}>
              Reset to default
            </button>
          </div>
        ))}
      {view.waiting && (
        <>
          <p className="note">{view.waiting}</p>
          {asking === "bring" ? (
            <ConfirmRow
              question="Bring them over?"
              detail={view.moves.join("; ")}
              action="Bring them over"
              busy={busy}
              onConfirm={() => void run(storageBringOver)}
              onCancel={() => setAsking(null)}
            />
          ) : (
            <div className="card-buttons">
              <button type="button" className="quiet" disabled={busy} onClick={() => setAsking("bring")}>
                Bring them over
              </button>
            </div>
          )}
        </>
      )}
      {status && (
        <p className="note" role="status">
          {status}
        </p>
      )}
    </section>
  );
}
