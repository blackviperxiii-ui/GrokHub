import { useEffect, useRef, useState } from "react";
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

/** A model to show, from a Pulse card's Open in Settings. A new `n` shows
 * it again. */
export interface LibraryFocus {
  id: string;
  n: number;
}

/** Download models: every catalog model by tab, with the best lineup for this PC.
 * `focus` opens the tab holding that model and marks its row; nothing
 * downloads until its Download button is pressed. */
export function ModelLibrary({ focus }: { focus?: LibraryFocus } = {}) {
  const [view, setView] = useState<LibraryView | null>(null);
  const [status, setStatus] = useState<LibraryStatus | null>(null);
  const [tab, setTab] = useState("Chat");
  const [error, setError] = useState<string | null>(null);
  /** The focus last applied, and whether this app's list has that model. */
  const [shown, setShown] = useState<{ id: string; n: number; found: boolean } | null>(null);
  const focusedRow = useRef<HTMLLIElement>(null);
  /** The focus `n` the row was already scrolled to and focused for. */
  const movedFor = useRef<number | null>(null);

  // Applied once the rows are in, so the right tab can be picked.
  if (view && focus && focus.n !== shown?.n) {
    const home = view.tabs.find((t) => t.rows.some((r) => r.id === focus.id));
    setShown({ id: focus.id, n: focus.n, found: Boolean(home) });
    if (home) setTab(home.label);
  }
  // Opened without a card (the palette, a menu pick): no row is marked.
  if (!focus && shown) setShown(null);

  useEffect(() => {
    // Once per card: effects run again each time a hidden page is shown,
    // and coming back to Settings mustn't jump to the row again.
    if (!shown?.found || movedFor.current === shown.n) return;
    const n = shown.n;
    // A frame later: Settings scrolls a newly opened section to the top in
    // its own effect, which runs after this one.
    const frame = requestAnimationFrame(() => {
      movedFor.current = n;
      const row = focusedRow.current;
      row?.scrollIntoView?.({ block: "center" });
      // Download or Resume when the row offers it; never Remove, which one
      // stray Enter would run. Otherwise the row itself.
      const target = row?.querySelector<HTMLElement>("[data-fetch]") ?? row;
      target?.focus({ preventScroll: true });
    });
    return () => cancelAnimationFrame(frame);
  }, [shown]);

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
          <button
            type="button"
            className="quiet"
            data-fetch
            onClick={() => void run(() => downloadModels([row.id]))}
          >
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
      {shown && !shown.found && <p className="note">That model isn't in this app's model list.</p>}
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
          const focused = shown?.found === true && shown.id === row.id;
          return (
            <li
              key={row.id}
              ref={focused ? focusedRow : undefined}
              tabIndex={focused ? -1 : undefined}
              aria-label={row.name}
              aria-current={focused ? "true" : undefined}
              className="local-library-row"
            >
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
