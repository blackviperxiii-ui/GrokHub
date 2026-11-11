import { useEffect, useEffectEvent, useRef, useState, type CSSProperties } from "react";
import {
  battlenetOpenFolder,
  battlenetPlay,
  getBattlenet,
  getLibrary,
  getMission,
  libraryCover,
  libraryPlay,
  type BattlenetGame,
  type CoverShape,
  type LibraryGame,
  type LibraryPage,
  type Mission,
} from "./api";
import { GameCompanion, type CompanionTarget } from "./GameCompanion";

export type LibraryTab = "all" | "installed" | "backlog" | "onObjective";

export const TAB_LABELS: Record<LibraryTab, string> = {
  all: "All",
  installed: "Installed",
  backlog: "Backlog",
  onObjective: "On objective",
};
const TABS: LibraryTab[] = ["all", "installed", "backlog", "onObjective"];

export const NO_STEAM = "Steam isn't installed on this computer, or its library folder wasn't found.";
export const NO_GAMES = "No Steam or Battle.net games are installed yet.";
export const NO_MATCH = "No games match.";
export const NO_OBJECTIVE = "No open mission names a game yet.";

export type LibraryTile =
  | { kind: "steam"; game: LibraryGame }
  | { kind: "battlenet"; game: BattlenetGame };

export type LibraryPick = { kind: "steam"; appId: number } | { kind: "battlenet"; product: string };

export function tileKey(t: LibraryTile): string {
  return t.kind === "steam" ? `steam:${t.game.appId}` : `battlenet:${t.game.product}`;
}

export function tileName(t: LibraryTile): string {
  return t.game.name;
}

export function tileSeed(t: LibraryTile): number {
  return t.kind === "steam" ? t.game.appId : t.game.seed;
}

/** Open mission words a game can be named in (title + checklist). */
export function objectiveText(mission: Mission | null | undefined): string {
  if (!mission) return "";
  const lines = [mission.title];
  for (const step of mission.steps) {
    if (step.status !== "cancelled") lines.push(step.text);
  }
  return lines.join("\n");
}

/** Whether `text` names the game, ignoring case. */
export function namesGame(text: string, name: string): boolean {
  const n = name.trim().toLowerCase();
  return n.length > 0 && text.toLowerCase().includes(n);
}

/** Installed has every file on disk; Backlog was never played; On objective
 * is named in the open mission. Battle.net sits out Backlog. */
export function filterTiles(
  steam: LibraryGame[],
  battlenet: BattlenetGame[],
  tab: LibraryTab,
  query: string,
  objective: string,
): LibraryTile[] {
  const q = query.trim().toLowerCase();
  const steamTiles: LibraryTile[] = steam
    .filter((g) => {
      if (tab === "installed") return g.installed;
      if (tab === "backlog") return g.playtimeMin === 0 && g.lastPlayed === 0;
      if (tab === "onObjective") return namesGame(objective, g.name);
      return true;
    })
    .filter((g) => !q || g.name.toLowerCase().includes(q))
    .map((game) => ({ kind: "steam" as const, game }));
  const bnetTiles: LibraryTile[] = battlenet
    .filter((g) => {
      if (tab === "backlog") return false;
      if (tab === "onObjective") return namesGame(objective, g.name);
      return true;
    })
    .filter((g) => !q || g.name.toLowerCase().includes(q))
    .map((game) => ({ kind: "battlenet" as const, game }));
  return [...steamTiles, ...bnetTiles];
}

/** Kept for existing tests: Steam-only filter. */
export function filterGames(games: LibraryGame[], tab: LibraryTab, query: string): LibraryGame[] {
  return filterTiles(games, [], tab === "onObjective" ? "all" : tab, query, "").flatMap((t) =>
    t.kind === "steam" ? [t.game] : [],
  );
}

/** "Never played", "45 min played" or "12.6 h played". */
export function playtimeLabel(minutes: number): string {
  if (minutes <= 0) return "Never played";
  if (minutes < 60) return `${minutes} min played`;
  return `${Math.round(minutes / 6) / 10} h played`;
}

const coverKey = (appId: number, shape: CoverShape) => `${appId}:${shape}`;

function initials(name: string): string {
  const words = name.split(/\s+/).filter((w) => /[\p{L}\p{N}]/u.test(w));
  return words
    .slice(0, 2)
    .map((w) => [...w][0] ?? "")
    .join("")
    .toUpperCase();
}

function Art({
  name,
  seed,
  src,
  shape,
}: {
  name: string;
  seed: number;
  src: string | null | undefined;
  shape: CoverShape;
}) {
  if (src) return <img className={`library-art ${shape}`} src={src} alt="" draggable={false} />;
  return (
    <div
      className={`library-art ${shape} placeholder`}
      style={{ "--art-hue": `${(seed * 47) % 360}` } as CSSProperties}
      aria-hidden="true"
    >
      <span>{initials(name)}</span>
    </div>
  );
}

function Tile({
  tile,
  src,
  picked,
  onPick,
  onWant,
}: {
  tile: LibraryTile;
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
      <button
        ref={ref}
        type="button"
        className="library-tile"
        aria-pressed={picked}
        aria-label={tileName(tile)}
        onClick={onPick}
      >
        <Art name={tileName(tile)} seed={tileSeed(tile)} src={src} shape="tall" />
        <span className="library-name">{tileName(tile)}</span>
      </button>
    </li>
  );
}

function pickOf(t: LibraryTile): LibraryPick {
  return t.kind === "steam"
    ? { kind: "steam", appId: t.game.appId }
    : { kind: "battlenet", product: t.game.product };
}

function samePick(a: LibraryPick | null, b: LibraryPick): boolean {
  if (!a) return false;
  if (a.kind === "steam" && b.kind === "steam") return a.appId === b.appId;
  if (a.kind === "battlenet" && b.kind === "battlenet") return a.product === b.product;
  return false;
}

function companionTarget(tile: LibraryTile): CompanionTarget {
  return tile.kind === "steam"
    ? { kind: "steam", appId: tile.game.appId, lastPlayed: tile.game.lastPlayed }
    : { kind: "battlenet", seed: tile.game.seed };
}

/** The Library page: Steam and Battle.net covers, with a companion frame. */
export function Library() {
  const [page, setPage] = useState<LibraryPage | null>(null);
  const [bnet, setBnet] = useState<BattlenetGame[]>([]);
  const [mission, setMission] = useState<Mission | null>(null);
  const [tab, setTab] = useState<LibraryTab>("all");
  const [query, setQuery] = useState("");
  const [picked, setPicked] = useState<LibraryPick | null>(null);
  const [covers, setCovers] = useState<Record<string, string | null>>({});
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const asked = useRef(new Set<string>());

  useEffect(() => {
    getLibrary().then(setPage, (err) => setNote(String(err)));
    getBattlenet().then(
      (p) => setBnet(p.games),
      () => setBnet([]),
    );
    getMission().then(
      (p) => setMission(p.mission),
      () => setMission(null),
    );
  }, []);

  function wantSteam(appId: number, shape: CoverShape) {
    const key = coverKey(appId, shape);
    if (asked.current.has(key)) return;
    asked.current.add(key);
    libraryCover(appId, shape).then(
      (src) => setCovers((now) => ({ ...now, [key]: src })),
      () => setCovers((now) => ({ ...now, [key]: null })),
    );
  }

  const objective = objectiveText(mission);
  const shown =
    page || bnet.length > 0 ? filterTiles(page?.games ?? [], bnet, tab, query, objective) : [];
  const tile = shown.find((t) => samePick(picked, pickOf(t))) ?? shown[0];

  const steamId = tile?.kind === "steam" ? tile.game.appId : undefined;
  const wantWide = useEffectEvent((appId: number) => wantSteam(appId, "wide"));
  useEffect(() => {
    if (steamId !== undefined) wantWide(steamId);
  }, [steamId]);

  async function playSteam(g: LibraryGame) {
    setBusy(true);
    try {
      setNote(await libraryPlay(g.appId));
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function playBnet(g: BattlenetGame) {
    setBusy(true);
    try {
      setNote(await battlenetPlay(g.product));
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function openBnetFolder(g: BattlenetGame) {
    setBusy(true);
    try {
      setNote(await battlenetOpenFolder(g.product));
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  const emptyLibrary =
    page !== null && !page.steamFound && bnet.length === 0
      ? NO_STEAM
      : page !== null && page.games.length === 0 && bnet.length === 0
        ? NO_GAMES
        : null;
  const emptyFilter =
    shown.length === 0 && (page !== null || bnet.length > 0)
      ? tab === "onObjective" && !objective.trim()
        ? NO_OBJECTIVE
        : tab === "onObjective"
          ? NO_MATCH
          : NO_MATCH
      : null;

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
      {page === null && bnet.length === 0 && !note && <p className="empty">Loading your games…</p>}
      {emptyLibrary && <p className="empty">{emptyLibrary}</p>}
      {emptyFilter && !emptyLibrary && <p className="empty">{emptyFilter}</p>}
      {tile && (
        <div className="library-body">
          <ul className="library-grid" aria-label="Games">
            {shown.map((t) => (
              <Tile
                key={tileKey(t)}
                tile={t}
                src={
                  t.kind === "steam" ? covers[coverKey(t.game.appId, "tall")] : undefined
                }
                picked={samePick(picked ?? pickOf(tile), pickOf(t))}
                onPick={() => setPicked(pickOf(t))}
                onWant={() => {
                  if (t.kind === "steam") wantSteam(t.game.appId, "tall");
                }}
              />
            ))}
          </ul>
          <aside className="library-detail card" aria-label="Game details">
            <Art
              name={tileName(tile)}
              seed={tileSeed(tile)}
              src={
                tile.kind === "steam" ? covers[coverKey(tile.game.appId, "wide")] : undefined
              }
              shape="wide"
            />
            <h2>{tileName(tile)}</h2>
            {tile.kind === "steam" && (
              <>
                <p className="card-status">{playtimeLabel(tile.game.playtimeMin)}</p>
                {tile.game.lastPlayed > 0 && (
                  <p className="card-status">
                    Last played {new Date(tile.game.lastPlayed * 1000).toLocaleDateString()}
                  </p>
                )}
                {!tile.game.installed && (
                  <p className="card-status">Steam is still downloading or updating it.</p>
                )}
                <button type="button" disabled={busy} onClick={() => void playSteam(tile.game)}>
                  Play
                </button>
              </>
            )}
            {tile.kind === "battlenet" && (
              <>
                <p className="card-status">
                  {tile.game.prefix ? "Battle.net in a Wine prefix" : "Battle.net"}
                </p>
                <div className="library-detail-actions">
                  <button type="button" disabled={busy} onClick={() => void playBnet(tile.game)}>
                    Play
                  </button>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void openBnetFolder(tile.game)}
                  >
                    Open folder
                  </button>
                </div>
              </>
            )}
            <GameCompanion key={tileKey(tile)} target={companionTarget(tile)} />
          </aside>
        </div>
      )}
    </section>
  );
}
