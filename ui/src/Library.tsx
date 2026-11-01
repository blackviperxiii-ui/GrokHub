import { useEffect, useEffectEvent, useRef, useState, type CSSProperties } from "react";
import { getLibrary, libraryCover, libraryPlay, type CoverShape, type LibraryGame, type LibraryPage } from "./api";

export type LibraryTab = "all" | "installed" | "backlog";

export const TAB_LABELS: Record<LibraryTab, string> = {
  all: "All",
  installed: "Installed",
  backlog: "Backlog",
};
const TABS: LibraryTab[] = ["all", "installed", "backlog"];

export const NO_STEAM = "Steam isn't installed on this computer, or its library folder wasn't found.";
export const NO_GAMES = "No Steam games are installed yet.";
export const NO_MATCH = "No games match.";

/** Installed has every file on disk; Backlog was never played. The search
 * matches the name, ignoring case. */
export function filterGames(games: LibraryGame[], tab: LibraryTab, query: string): LibraryGame[] {
  const q = query.trim().toLowerCase();
  return games
    .filter((g) => {
      if (tab === "installed") return g.installed;
      if (tab === "backlog") return g.playtimeMin === 0 && g.lastPlayed === 0;
      return true;
    })
    .filter((g) => !q || g.name.toLowerCase().includes(q));
}

/** "Never played", "45 min played" or "12.6 h played". */
export function playtimeLabel(minutes: number): string {
  if (minutes <= 0) return "Never played";
  if (minutes < 60) return `${minutes} min played`;
  return `${Math.round(minutes / 6) / 10} h played`;
}

const coverKey = (appId: number, shape: CoverShape) => `${appId}:${shape}`;

/** Up to two letters for the painted cover. */
function initials(name: string): string {
  const words = name.split(/\s+/).filter((w) => /[\p{L}\p{N}]/u.test(w));
  return words
    .slice(0, 2)
    .map((w) => [...w][0] ?? "")
    .join("")
    .toUpperCase();
}

/** The art, or a painted placeholder in a colour picked from the app id. */
function Art({ game, src, shape }: { game: LibraryGame; src: string | null | undefined; shape: CoverShape }) {
  if (src) return <img className={`library-art ${shape}`} src={src} alt="" draggable={false} />;
  return (
    <div
      className={`library-art ${shape} placeholder`}
      style={{ "--art-hue": `${(game.appId * 47) % 360}` } as CSSProperties}
      aria-hidden="true"
    >
      <span>{initials(game.name)}</span>
    </div>
  );
}

/** One grid tile. It asks for its cover once it scrolls near the view. */
function Tile({
  game,
  src,
  picked,
  onPick,
  onWant,
}: {
  game: LibraryGame;
  src: string | null | undefined;
  picked: boolean;
  onPick: () => void;
  onWant: () => void;
}) {
  const ref = useRef<HTMLButtonElement>(null);
  const want = useEffectEvent(onWant);
  useEffect(() => {
    const el = ref.current;
    if (!el || typeof IntersectionObserver === "undefined") {
      want();
      return;
    }
    const seen = new IntersectionObserver(
      (entries) => {
        if (entries.some((e) => e.isIntersecting)) {
          want();
          seen.disconnect();
        }
      },
      { rootMargin: "400px" },
    );
    seen.observe(el);
    return () => seen.disconnect();
  }, []);
  return (
    <li>
      <button ref={ref} type="button" className="library-tile" aria-pressed={picked} aria-label={game.name} onClick={onPick}>
        <Art game={game} src={src} shape="tall" />
        <span className="library-name">{game.name}</span>
      </button>
    </li>
  );
}

/** The Library page: this computer's Steam games as a cover grid, with Play. */
export function Library() {
  const [page, setPage] = useState<LibraryPage | null>(null);
  const [tab, setTab] = useState<LibraryTab>("all");
  const [query, setQuery] = useState("");
  const [picked, setPicked] = useState<number | null>(null);
  const [covers, setCovers] = useState<Record<string, string | null>>({});
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const asked = useRef(new Set<string>());

  useEffect(() => {
    getLibrary().then(setPage, (err) => setNote(String(err)));
  }, []);

  function want(appId: number, shape: CoverShape) {
    const key = coverKey(appId, shape);
    if (asked.current.has(key)) return;
    asked.current.add(key);
    libraryCover(appId, shape).then(
      (src) => setCovers((now) => ({ ...now, [key]: src })),
      () => setCovers((now) => ({ ...now, [key]: null })),
    );
  }

  const shown = page ? filterGames(page.games, tab, query) : [];
  const game = shown.find((g) => g.appId === picked) ?? shown[0];

  // The detail column's header art, once per game picked.
  const gameId = game?.appId;
  const wantWide = useEffectEvent((appId: number) => want(appId, "wide"));
  useEffect(() => {
    if (gameId !== undefined) wantWide(gameId);
  }, [gameId]);

  async function play(g: LibraryGame) {
    setBusy(true);
    try {
      setNote(await libraryPlay(g.appId));
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="library" aria-label="Library">
      <div className="library-head">
        <div className="switch" role="group" aria-label="Library view">
          {TABS.map((t) => (
            <button key={t} type="button" aria-pressed={tab === t} onClick={() => setTab(t)}>
              {TAB_LABELS[t]}
            </button>
          ))}
        </div>
        <input
          type="search"
          aria-label="Search games"
          placeholder="Search games"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
      </div>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      {page === null && !note && <p className="empty">Loading your games…</p>}
      {page && !page.steamFound && <p className="empty">{NO_STEAM}</p>}
      {page && page.steamFound && page.games.length === 0 && <p className="empty">{NO_GAMES}</p>}
      {page && page.games.length > 0 && shown.length === 0 && <p className="empty">{NO_MATCH}</p>}
      {game && (
        <div className="library-body">
          <ul className="library-grid" aria-label="Games">
            {shown.map((g) => (
              <Tile
                key={g.appId}
                game={g}
                src={covers[coverKey(g.appId, "tall")]}
                picked={g.appId === game.appId}
                onPick={() => setPicked(g.appId)}
                onWant={() => want(g.appId, "tall")}
              />
            ))}
          </ul>
          <aside className="library-detail card" aria-label="Game details">
            <Art game={game} src={covers[coverKey(game.appId, "wide")]} shape="wide" />
            <h2>{game.name}</h2>
            <p className="card-status">{playtimeLabel(game.playtimeMin)}</p>
            {game.lastPlayed > 0 && (
              <p className="card-status">Last played {new Date(game.lastPlayed * 1000).toLocaleDateString()}</p>
            )}
            {!game.installed && <p className="card-status">Steam is still downloading or updating it.</p>}
            <button type="button" disabled={busy} onClick={() => void play(game)}>
              Play
            </button>
          </aside>
        </div>
      )}
    </section>
  );
}
