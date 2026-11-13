import { useEffect, useState } from "react";
import {
  downloadModels,
  getModelLibrary,
  onModelLibraryEvent,
  pauseModelDownloads,
  removeLibraryModel,
  type LibraryRow,
  type LibraryStatus,
  type LibraryView,
} from "./api";

/** Download models: every catalog model by tab, with the best lineup for this PC. */
export function ModelLibrary() {
  const [view, setView] = useState<LibraryView | null>(null);
  const [status, setStatus] = useState<LibraryStatus | null>(null);
  const [tab, setTab] = useState("Chat");
  const [error, setError] = useState<string | null>(null);

  const show = (next: LibraryView) => {
    setView(next);
    setStatus(next.status);
  };
  const run = async (act: () => Promise<LibraryView>) => {
    setError(null);
    try {
      show(await act());
    } catch (err) {
      setError(String(err));
    }
  };

  useEffect(() => {
    let stop: (() => void) | undefined;
    let dropped = false;
    getModelLibrary().then(show, (err: unknown) => setError(String(err)));
    void onModelLibraryEvent((next) => {
      setStatus(next);
      // The worker stopped: rows changed on disk, so read them again.
      if (!next.running) getModelLibrary().then(show, () => undefined);
    }).then((un) => {
      if (dropped) un();
      else stop = un;
    });
    return () => {
      dropped = true;
      stop?.();
    };
  }, []);

  if (!view || !status) {
    return (
      <div className="local-library" aria-label="Download models">
        <h3>Download models</h3>
        <p className="note">{error ?? "Checking which models fit this PC…"}</p>
      </div>
    );
  }
  const open = view.tabs.find((t) => t.label === tab) ?? view.tabs[0];

  const action = (row: LibraryRow) => {
    switch (row.state) {
      case "download":
      case "resume":
        return (
          <button type="button" className="quiet" onClick={() => void run(() => downloadModels([row.id]))}>
            {row.state === "resume" ? `Resume ${row.name}` : `Download ${row.name}`}
          </button>
        );
      case "downloaded":
        return (
          <button type="button" className="quiet" onClick={() => void run(() => removeLibraryModel(row.id))}>
            Remove {row.name}
          </button>
        );
      case "downloading":
        return status.job && <p className="note local-draft-progress">{status.job.line}</p>;
      case "queued":
        return <p className="note">Queued.</p>;
      case "inUse":
        return <p className="note">In use.</p>;
      case "wontRun":
        return null;
    }
  };

  return (
    <div className="local-library" aria-label="Download models">
      <h3>Download models</h3>
      <div className="local-library-tabs" role="tablist" aria-label="Model kinds">
        {view.tabs.map((t) => (
          <button
            key={t.label}
            type="button"
            role="tab"
            aria-selected={t.label === open.label}
            className={t.label === open.label ? "" : "quiet"}
            onClick={() => setTab(t.label)}
          >
            {t.label} ({t.rows.length})
          </button>
        ))}
      </div>
      <ul aria-label={`${open.label} models`}>
        {open.rows.map((row) => {
          const failed = status.errors[row.id];
          return (
            <li key={row.id} aria-label={row.name} className="local-library-row">
              <p>{row.line}</p>
              <p className="note">{row.about}</p>
              {action(row)}
              {failed && (
                <div className="local-library-row-error" role="alert">
                  <p className="note error">{failed.message}</p>
                  <p className="note">{failed.detail}</p>
                </div>
              )}
            </li>
          );
        })}
      </ul>
      {open.empty && <p className="note">{open.empty}</p>}
      {status.job ? (
        <div className="local-library-queue">
          <p className="note">
            {status.queued > 0 ? `${status.queued} more after this one.` : "Last one in the queue."}
          </p>
          <button type="button" className="quiet" onClick={() => void pauseModelDownloads()}>
            Pause downloads
          </button>
        </div>
      ) : (
        <div className="local-library-queue">
          {view.lineup && <p className="note">Best lineup for this PC: {view.lineup.line}</p>}
          {view.lineup && view.lineup.ids.length > 0 ? (
            <button type="button" onClick={() => void run(() => downloadModels(view.lineup?.ids ?? []))}>
              Download lineup
            </button>
          ) : (
            view.everything && (
              <button
                type="button"
                className="quiet"
                onClick={() => void run(() => downloadModels(view.everything?.ids ?? []))}
              >
                Download everything that fits ({view.everything.line})
              </button>
            )
          )}
        </div>
      )}
      {status.message && (
        <p className="note" role="status">
          {status.message}
        </p>
      )}
      {error && (
        <p className="note error" role="status">
          {error}
        </p>
      )}
    </div>
  );
}
