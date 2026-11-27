import { useEffect, useState, type FormEvent, type KeyboardEvent, type MouseEvent } from "react";
import {
  getSessionsBoard,
  sessionsBoardPin,
  sessionsBoardReshape,
  sessionsBoardStamp,
  type SessionsBoardPage,
  type SessionsFilter,
  type SessionsTile,
} from "./api";
import { openExternal } from "./external";

export const SESSIONS_EMPTY =
  "No conversations match this board yet. Chats from the last 72 hours land here by rules and pins.";
export const SESSIONS_HELP =
  "Tiles are conversations placed by rules and pins — not by model classification. Pin a tile to a column, filter personally, or ask the Board agent to reshape columns.";
export const STAMP_EVERY_MS = 3000;

const FILTERS: { id: SessionsFilter; label: string }[] = [
  { id: "everyone", label: "Everyone" },
  { id: "involvingMe", label: "Involving me" },
];

const RUN_LABEL: Record<string, string> = {
  waiting: "Needs input",
  failed: "Stuck",
  running: "Working",
  idle: "Idle",
};

export function SessionsBoard({ onOpenChat }: { onOpenChat: (id: string) => void }) {
  const [page, setPage] = useState<SessionsBoardPage | null>(null);
  const [filter, setFilter] = useState<SessionsFilter>("everyone");
  const [query, setQuery] = useState("");
  // The query the board shows; typed text counts only once applied.
  const [applied, setApplied] = useState("");
  const [agent, setAgent] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  function refresh(nextFilter = filter, nextQuery = applied) {
    return getSessionsBoard(nextFilter, nextQuery).then(
      (next) => {
        setPage(next);
        setNote(null);
      },
      (err) => setNote(String(err)),
    );
  }

  useEffect(() => {
    getSessionsBoard(filter, applied).then(
      (next) => setPage(next),
      (err) => setNote(String(err)),
    );
    // First open only — filter/query changes go through Apply / filter buttons.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const stamp = page?.stamp;
  useEffect(() => {
    if (stamp === undefined) return;
    const timer = window.setInterval(() => {
      sessionsBoardStamp().then(
        (current) => {
          if (current !== stamp) {
            getSessionsBoard(filter, applied).then(setPage, () => {});
          }
        },
        () => {},
      );
    }, STAMP_EVERY_MS);
    return () => window.clearInterval(timer);
  }, [stamp, filter, applied]);

  async function applyFilter(next: SessionsFilter) {
    setFilter(next);
    setBusy(true);
    try {
      await refresh(next, applied);
    } finally {
      setBusy(false);
    }
  }

  async function applyQuery(event: FormEvent) {
    event.preventDefault();
    setApplied(query);
    setBusy(true);
    try {
      await refresh(filter, query);
    } finally {
      setBusy(false);
    }
  }

  async function pin(tile: SessionsTile, columnId: string) {
    setBusy(true);
    try {
      setPage(await sessionsBoardPin(tile.id, columnId, filter, applied));
      setNote(columnId ? `Pinned to ${columnId}.` : "Pin cleared.");
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function askAgent(event: FormEvent) {
    event.preventDefault();
    if (!agent.trim() || busy) return;
    setBusy(true);
    try {
      const result = await sessionsBoardReshape(agent, filter, applied);
      setPage(result.page);
      setNote(result.note);
      setAgent("");
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  // Tauri ignores target="_blank"; open the PR in the system browser.
  function openPr(event: MouseEvent<HTMLAnchorElement>, url: string) {
    event.preventDefault();
    void openExternal(url).catch((err) => setNote(String(err)));
  }

  function onAgentKey(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Enter") {
      event.preventDefault();
      void askAgent(event as unknown as FormEvent);
    }
  }

  return (
    <div className="sessions-board" aria-label="Sessions board">
      <p className="sessions-help">{SESSIONS_HELP}</p>
      <div className="sessions-toolbar">
        <div className="sessions-filters" role="group" aria-label="Personal filter">
          {FILTERS.map((f) => (
            <button
              key={f.id}
              type="button"
              className={filter === f.id ? "active" : undefined}
              aria-pressed={filter === f.id}
              disabled={busy}
              onClick={() => void applyFilter(f.id)}
            >
              {f.label}
            </button>
          ))}
        </div>
        <form className="sessions-query" onSubmit={applyQuery}>
          <label className="sr-only" htmlFor="sessions-query">
            Filter by title
          </label>
          <input
            id="sessions-query"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Filter by title"
            disabled={busy}
          />
          <button type="submit" disabled={busy}>
            Apply
          </button>
        </form>
      </div>
      <form className="sessions-agent" onSubmit={askAgent}>
        <label htmlFor="sessions-agent">Board agent</label>
        <input
          id="sessions-agent"
          value={agent}
          onChange={(e) => setAgent(e.target.value)}
          onKeyDown={onAgentKey}
          placeholder="rename working to Active · move done before needs-input · reset columns"
          disabled={busy}
          autoComplete="off"
        />
        <button type="submit" disabled={busy || !agent.trim()}>
          Reshape
        </button>
      </form>
      {note ? (
        <p className="sessions-note" role="status">
          {note}
        </p>
      ) : null}
      {page ? (
        <>
          <p className="sessions-count">
            Showing {page.shown} of {page.total}
          </p>
          <div className="board-columns sessions-columns">
            {page.columns.map((col) => (
              <section key={col.id} className="board-column" aria-label={col.label}>
                <h2>
                  {col.label}
                  <span className="sessions-rule">{col.rule}</span>
                </h2>
                <div className="rows">
                  {col.tiles.length === 0 ? (
                    <p className="sessions-col-empty">None</p>
                  ) : (
                    col.tiles.map((tile) => (
                      <article key={tile.id} className="card board-card sessions-tile">
                        <button
                          type="button"
                          className="sessions-open"
                          onClick={() => onOpenChat(tile.id)}
                        >
                          <strong>{tile.title || "Untitled"}</strong>
                          <span className={`sessions-run run-${tile.run}`}>
                            {RUN_LABEL[tile.run] ?? tile.run}
                          </span>
                          {tile.preview ? <p>{tile.preview}</p> : null}
                        </button>
                        {tile.prs.length > 0 ? (
                          <ul className="sessions-prs">
                            {tile.prs.map((pr) => (
                              <li key={pr.url}>
                                <a
                                  href={pr.url}
                                  target="_blank"
                                  rel="noreferrer"
                                  onClick={(e) => openPr(e, pr.url)}
                                >
                                  PR · {pr.state}
                                </a>
                              </li>
                            ))}
                          </ul>
                        ) : null}
                        <div className="sessions-pin-row">
                          <label className="sr-only" htmlFor={`pin-${tile.id}`}>
                            Pin {tile.title} to column
                          </label>
                          <select
                            id={`pin-${tile.id}`}
                            value={tile.pinned ? tile.columnId : ""}
                            disabled={busy}
                            onChange={(e) => void pin(tile, e.target.value)}
                          >
                            <option value="">Auto (rules)</option>
                            {page.columns.map((c) => (
                              <option key={c.id} value={c.id}>
                                Pin · {c.label}
                              </option>
                            ))}
                          </select>
                        </div>
                      </article>
                    ))
                  )}
                </div>
              </section>
            ))}
          </div>
          {page.shown === 0 ? <p className="sessions-empty">{SESSIONS_EMPTY}</p> : null}
        </>
      ) : (
        <p className="sessions-empty">{busy ? "Loading…" : SESSIONS_EMPTY}</p>
      )}
    </div>
  );
}
