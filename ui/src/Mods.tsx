import { useEffect, useEffectEvent, useRef, useState } from "react";
import {
  modsBrowse,
  modsForYou,
  modsGames,
  modsHideGame,
  modsNewFinds,
  modsNotForMe,
  modsSearch,
  openUrl,
  type ModGame,
  type ModGamesView,
  type ModList,
  type ModPage,
  type ModRow,
  type ModSuggestion,
} from "./api";

export type ModsTab = ModList | "newFinds";

export const TAB_LABELS: Record<ModsTab, string> = {
  popular: "Popular",
  trending: "Trending",
  newFinds: "New finds",
  updated: "Updated",
};
const TABS: ModsTab[] = ["popular", "trending", "newFinds", "updated"];

export const SUBTITLE = "Safe, sanctioned mods for the games on this PC.";
export const NO_GAMES = "No moddable games found";
export const NOTHING = "Nothing here yet.";
export const INSTALL_SOON = "Installing comes in the next update.";
/** How long typing must pause before a search goes out. */
export const QUERY_WAIT_MS = 400;

/** "1.2M", "34K" or "512". */
export function shortCount(n: number): string {
  if (n >= 1_000_000) return `${Math.round(n / 100_000) / 10}M`;
  if (n >= 1_000) return `${Math.round(n / 100) / 10}K`;
  return String(n);
}

/** One mod row: name, author, summary, counts, Open page, and Install
 * (disabled until the install sheet ships). */
function Row({ mod }: { mod: ModRow }) {
  return (
    <li className="mods-row card">
      {mod.iconUrl ? (
        <img className="mods-icon" src={mod.iconUrl} alt="" draggable={false} />
      ) : (
        <div className="mods-icon placeholder" aria-hidden="true" />
      )}
      <div className="mods-row-body">
        <h3>{mod.name}</h3>
        {mod.author && <p className="card-status">by {mod.author}</p>}
        {mod.summary && <p className="mods-summary">{mod.summary}</p>}
        <p className="card-status">
          {shortCount(mod.downloads)} downloads
          {mod.updated > 0 && ` · Updated ${new Date(mod.updated * 1000).toLocaleDateString()}`}
        </p>
      </div>
      <div className="mods-row-actions">
        <button type="button" onClick={() => void openUrl(mod.pageUrl).catch(() => {})}>
          Open page
        </button>
        <button type="button" disabled title={INSTALL_SOON}>
          Install
        </button>
      </div>
    </li>
  );
}

/** The For you row: up to five cards, each with View and Not for me. */
function ForYou({
  cards,
  onView,
  onNotForMe,
}: {
  cards: ModSuggestion[];
  onView: (s: ModSuggestion) => void;
  onNotForMe: (s: ModSuggestion) => void;
}) {
  if (cards.length === 0) return null;
  return (
    <section className="mods-foryou" aria-label="For you">
      <h2>
        For you <span className="card-status">Picked from what you play</span>
      </h2>
      <ul>
        {cards.map((s) => (
          <li key={`${s.source}:${s.mod.id}`} className="card">
            <p className="card-status">{s.gameName}</p>
            <h3>{s.mod.name}</h3>
            <p className="card-status">{s.why}</p>
            <div className="mods-row-actions">
              <button type="button" onClick={() => onView(s)}>
                View
              </button>
              <button type="button" onClick={() => onNotForMe(s)}>
                Not for me
              </button>
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}

/** The Mods page: the moddable games on this PC, and for the picked one,
 * Popular, Trending, New finds and Updated per source, a search, and For you. */
export function Mods() {
  const [view, setView] = useState<ModGamesView | null>(null);
  const [forYou, setForYou] = useState<ModSuggestion[]>([]);
  const [pickedId, setPickedId] = useState<string | null>(null);
  const [sourceId, setSourceId] = useState<string | null>(null);
  const [tab, setTab] = useState<ModsTab>("popular");
  const [query, setQuery] = useState("");
  const [asked, setAsked] = useState("");
  const [pageNo, setPageNo] = useState(0);
  const [got, setGot] = useState<{ key: string; page: ModPage } | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [showCant, setShowCant] = useState(false);
  const ask = useRef(0);

  function loadRail() {
    modsGames().then(setView, (err) => setNote(String(err)));
  }
  function loadForYou() {
    modsForYou().then(setForYou, () => setForYou([]));
  }
  const loadAll = useEffectEvent(() => {
    loadRail();
    loadForYou();
  });
  useEffect(() => loadAll(), []);
  const reloadRail = useEffectEvent(() => loadRail());

  const game: ModGame | undefined = view?.games.find((g) => g.id === pickedId) ?? view?.games[0];
  const source = game?.sources.find((s) => s.id === sourceId) ?? game?.sources[0];

  // Typing waits for a pause before it searches.
  useEffect(() => {
    const t = setTimeout(() => {
      setAsked(query.trim());
      setPageNo(0);
    }, QUERY_WAIT_MS);
    return () => clearTimeout(t);
  }, [query]);

  const gameId = game?.id;
  const src = source?.id;
  const key = `${gameId}|${src}|${tab}|${asked}|${pageNo}`;
  // Until the answer for what is picked now comes, the list shows Looking.
  const page = got?.key === key ? got.page : null;
  useEffect(() => {
    if (!gameId || !src) return;
    const n = ++ask.current;
    const asking = asked
      ? modsSearch(gameId, src, asked, pageNo)
      : tab === "newFinds"
        ? modsNewFinds(gameId, src, pageNo)
        : modsBrowse(gameId, src, tab, pageNo);
    asking.then(
      (p) => {
        if (n !== ask.current) return;
        setGot({ key, page: p });
        // Opening New finds clears its count on the rail.
        if (!asked && tab === "newFinds") reloadRail();
      },
      (err) => {
        if (n === ask.current) setGot({ key, page: { mods: [], page: pageNo, more: false, note: String(err), cached: false } });
      },
    );
  }, [gameId, src, tab, asked, pageNo, key]);

  function pick(id: string) {
    setPickedId(id);
    setSourceId(null);
    setPageNo(0);
  }

  function viewSuggestion(s: ModSuggestion) {
    setPickedId(s.game);
    setSourceId(s.source);
    setTab(s.tab);
    setQuery("");
    setAsked("");
    setPageNo(0);
  }

  async function notForMe(s: ModSuggestion) {
    setForYou((now) => now.filter((x) => x !== s));
    await modsNotForMe(s.source, s.mod.id).catch((err) => setNote(String(err)));
    loadForYou();
  }

  async function hideSuggestions(g: ModGame) {
    await modsHideGame(g.id, !g.suggestionsHidden).catch((err) => setNote(String(err)));
    loadRail();
    loadForYou();
  }

  const subtitle = [SUBTITLE, view?.status].filter(Boolean).join(" ");

  return (
    <section className="mods" aria-label="Mods">
      <p className="card-status">{subtitle}</p>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      {view === null && !note && <p className="empty">Loading your games…</p>}
      {view && view.games.length === 0 && view.cant.length === 0 && <p className="empty">{NO_GAMES}</p>}
      {view && (view.games.length > 0 || view.cant.length > 0) && (
        <div className="mods-body">
          <nav className="mods-rail" aria-label="Moddable games">
            {view.games.length === 0 && <p className="empty">{NO_GAMES}</p>}
            <ul>
              {view.games.map((g) => (
                <li key={g.id}>
                  <button type="button" aria-pressed={g.id === game?.id} onClick={() => pick(g.id)}>
                    <span>{g.name}</span>
                    {g.newCount > 0 && <span className="mods-badge">{g.newCount} new</span>}
                  </button>
                </li>
              ))}
            </ul>
            {view.cant.length > 0 && (
              <div className="mods-cant">
                <button type="button" aria-expanded={showCant} onClick={() => setShowCant((s) => !s)}>
                  Can't mod ({view.cant.length})
                </button>
                {showCant && (
                  <ul aria-label="Can't mod">
                    {view.cant.map((c) => (
                      <li key={c.key}>
                        <span>{c.name}</span>
                        <span className="card-status">{c.reason}</span>
                      </li>
                    ))}
                  </ul>
                )}
              </div>
            )}
          </nav>
          <div className="mods-main">
            <ForYou cards={forYou} onView={viewSuggestion} onNotForMe={(s) => void notForMe(s)} />
            {game && (
              <>
                <div className="mods-head">
                  <div>
                    <h2>{game.name}</h2>
                    <p className="card-status">{game.modeNote}</p>
                  </div>
                  <button type="button" onClick={() => void hideSuggestions(game)}>
                    {game.suggestionsHidden ? "Show suggestions for this game" : "Hide suggestions for this game"}
                  </button>
                </div>
                <div className="mods-head">
                  <div className="switch" role="group" aria-label="Mod list">
                    {TABS.map((t) => (
                      <button
                        key={t}
                        type="button"
                        aria-pressed={!asked && tab === t}
                        onClick={() => {
                          setTab(t);
                          setQuery("");
                          setAsked("");
                          setPageNo(0);
                        }}
                      >
                        {TAB_LABELS[t]}
                      </button>
                    ))}
                  </div>
                  {game.sources.length > 1 && (
                    <div className="switch" role="group" aria-label="Source">
                      {game.sources.map((s) => (
                        <button
                          key={s.id}
                          type="button"
                          aria-pressed={s.id === source?.id}
                          onClick={() => {
                            setSourceId(s.id);
                            setPageNo(0);
                          }}
                        >
                          {s.label}
                        </button>
                      ))}
                    </div>
                  )}
                  <input
                    type="search"
                    aria-label={`Search ${game.name} mods`}
                    placeholder="Search mods"
                    value={query}
                    onChange={(e) => setQuery(e.target.value)}
                  />
                </div>
                {page === null && <p className="empty">Looking…</p>}
                {page?.note && <p className="note">{page.note}</p>}
                {page && page.mods.length === 0 && !page.note && <p className="empty">{NOTHING}</p>}
                {page && page.mods.length > 0 && (
                  <ul className="mods-list" aria-label="Mods">
                    {page.mods.map((m) => (
                      <Row key={m.id} mod={m} />
                    ))}
                  </ul>
                )}
                {page && (page.more || pageNo > 0) && (
                  <div className="mods-pager">
                    <button type="button" disabled={pageNo === 0} onClick={() => setPageNo((p) => p - 1)}>
                      Previous
                    </button>
                    <span className="card-status">
                      Page {pageNo + 1} · {page.mods.length} shown
                    </span>
                    <button type="button" disabled={!page.more} onClick={() => setPageNo((p) => p + 1)}>
                      Next
                    </button>
                  </div>
                )}
              </>
            )}
          </div>
        </div>
      )}
    </section>
  );
}
