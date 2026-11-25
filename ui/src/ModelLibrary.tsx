import { useEffect, useRef, useState } from "react";
import {
  downloadHfGguf,
  downloadModels,
  getModelLibrary,
  listHfGgufFiles,
  onModelLibraryEvent,
  pauseModelDownloads,
  removeLibraryModel,
  searchHfGguf,
  type HfGgufFile,
  type HfGgufHit,
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
  const [hfQuery, setHfQuery] = useState("");
  const [hfHits, setHfHits] = useState<HfGgufHit[]>([]);
  const [hfRepo, setHfRepo] = useState<string | null>(null);
  const [hfFiles, setHfFiles] = useState<HfGgufFile[]>([]);
  const [hfNote, setHfNote] = useState<string | null>(null);
  const [hfBusy, setHfBusy] = useState(false);

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

  const searchHf = async () => {
    setHfBusy(true);
    setHfNote(null);
    setHfRepo(null);
    setHfFiles([]);
    try {
      setHfHits(await searchHfGguf(hfQuery.trim()));
      if (!hfQuery.trim()) setHfNote(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setHfBusy(false);
    }
  };

  const openHfRepo = async (repo: string) => {
    setHfBusy(true);
    setHfNote(null);
    setHfRepo(repo);
    try {
      setHfFiles(await listHfGgufFiles(repo));
    } catch (err) {
      setError(String(err));
      setHfFiles([]);
    } finally {
      setHfBusy(false);
    }
  };

  const downloadHf = async (repo: string, path: string) => {
    setHfBusy(true);
    setHfNote(null);
    try {
      setHfNote(await downloadHfGguf(repo, path));
    } catch (err) {
      setError(String(err));
    } finally {
      setHfBusy(false);
    }
  };

  return (
    <div className="local-library" aria-label="Download models">
      <h3>Download models</h3>
      {shown && !shown.found && <p className="note">That model isn't in this app's model list.</p>}
      <div className="hf-browse" aria-label="Search Hugging Face GGUF">
        <p className="note">
          Search Hugging Face for GGUF quants. Downloads join Auto only — there is no model picker
          in chat.
        </p>
        <div className="hf-browse-search">
          <input
            type="search"
            aria-label="Search Hugging Face for GGUF models"
            placeholder="Search GGUF models"
            value={hfQuery}
            onChange={(e) => setHfQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") void searchHf();
            }}
          />
          <button type="button" className="quiet" disabled={hfBusy} onClick={() => void searchHf()}>
            Search
          </button>
        </div>
        {hfHits.length > 0 && (
          <ul aria-label="Hugging Face GGUF results">
            {hfHits.map((hit) => (
              <li key={hit.repo} className="local-library-row">
                <p>{hit.repo}</p>
                <p className="note">
                  {hit.downloads.toLocaleString()} downloads
                  {hit.gated ? " · gated" : ""}
                </p>
                <button
                  type="button"
                  className="quiet"
                  disabled={hfBusy}
                  onClick={() => void openHfRepo(hit.repo)}
                >
                  Show quants
                </button>
              </li>
            ))}
          </ul>
        )}
        {hfRepo && (
          <div className="hf-browse-files" aria-label={`Quants in ${hfRepo}`}>
            <h4>{hfRepo}</h4>
            {hfFiles.length === 0 ? (
              <p className="note">{hfBusy ? "Reading quants…" : "No servable GGUF files in this repo."}</p>
            ) : (
              <ul>
                {hfFiles.map((file) => (
                  <li key={file.paths.join("|")} className="local-library-row">
                    <p>
                      {file.label} · {file.size}{" "}
                      <span className={`badge hf-fit-${file.fitKind}`}>{file.fit}</span>
                    </p>
                    <button
                      type="button"
                      className="quiet"
                      disabled={hfBusy || file.fitKind === "no"}
                      onClick={() => void downloadHf(hfRepo, file.paths[0] ?? "")}
                    >
                      Download into Auto
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </div>
        )}
        {hfNote && (
          <p className="note" role="status">
            {hfNote}
          </p>
        )}
      </div>
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
              <p>
                <span className="local-library-job">{row.useCase}</span>
                {row.line}
              </p>
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
