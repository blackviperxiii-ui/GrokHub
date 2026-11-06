import { useEffect, useState, type ReactNode } from "react";
import {
  getHome,
  libraryPlay,
  openUrl,
  pulseAct,
  type HomePage,
  type HomeSlot,
  type HomeTile,
  type QuickChip,
  type SlotSize,
  type TileAction,
  type TileView,
  type Xp,
} from "./api";
import { ChipRow, useChipRow } from "./Chips";
import { COMPANION_NAME } from "./brand";
import { artUrl } from "./nyx";
import type { View } from "./Palette";
import type { Section } from "./sections";

export const EDIT_NOTE = "Edit Home in the cabin to change these tiles.";
export const NO_STEPS = `Nothing on the list yet. Ask ${COMPANION_NAME} to plan something.`;

const PRESETS: Record<string, string> = {
  mission_control: "Mission Control",
  streamer: "Streamer/Social",
  performance: "Performance/Tuning",
};

/** `2340` as `2,340`, as the cabin writes XP. */
export function grouped(n: number): string {
  return n.toLocaleString("en-US");
}

/** "Lvl 3" and "1,240 XP" over a bar filled to the level's progress. */
export function XpBar({ xp, full }: { xp: Xp; full?: boolean }) {
  const pct = Math.round(Math.min(1, Math.max(0, xp.frac)) * 100);
  return (
    <div className="xp-bar">
      <div className="xp-line">
        <span className="xp-level">Lvl {xp.level}</span>
        <span className="xp-amount">
          {full ? `${grouped(xp.xp)} / ${grouped(xp.nextLevelXp)} XP` : `${grouped(xp.xp)} XP`}
        </span>
      </div>
      <div
        className="xp-track"
        role="progressbar"
        aria-label={`Level ${xp.level}`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={pct}
        title={xp.today > 0 ? `+${grouped(xp.today)} XP today` : undefined}
      >
        <div className="xp-fill" style={{ width: `${pct}%` }} />
      </div>
    </div>
  );
}

/** Seven boxes, today last: filled for a day that finished a task. */
export function Streak({ days }: { days: boolean[] }) {
  const count = days.filter(Boolean).length;
  return (
    <div className="home-streak">
      <div className="xp-line">
        <span className="xp-level">Weekly streak</span>
        <span className="xp-amount">
          {count} {count === 1 ? "DAY" : "DAYS"}
        </span>
      </div>
      <ol className="streak-days" aria-label={`${count} of the last 7 days finished a task`}>
        {days.map((on, i) => (
          <li key={i} className={on ? "on" : i === days.length - 1 ? "today" : ""} />
        ))}
      </ol>
    </div>
  );
}

export interface HomeRoutes {
  onOpen: (view: View) => void;
  onOpenChat: (id: string) => void;
  /** Puts the text in the chat box; nothing is sent. */
  onAsk: (text: string) => void;
  onSettings: (section: Section) => void;
}

/** The pages a tile may open; anything else stays put. */
const VIEWS: readonly string[] = [
  "objectives",
  "chat",
  "history",
  "pulse",
  "journal",
  "automations",
  "memory",
  "connectors",
  "library",
  "workboard",
];

/** Where a feature's switch lives until it can be turned on from the tile. */
const FEATURE_SECTION: Record<string, Section> = { net_log: "privacy", local_ai: "localAi" };

/** How much of a tile each size shows (core's `fit`). */
const FIT: Record<SlotSize, { rows: number; buttons: number }> = {
  small: { rows: 0, buttons: 0 },
  medium: { rows: 4, buttons: 2 },
  large: { rows: 8, buttons: 3 },
};

/** Runs a tile action through Home's routes. On a paired browser nothing
 * starts on the computer (Play), and links open in the browser itself. */
export function runAction(a: TileAction, go: HomeRoutes, remote: boolean, reload: () => void) {
  switch (a.kind) {
    case "ask":
      return go.onAsk(a.ask);
    case "openChat":
      return go.onOpenChat(a.id);
    case "nav":
      if (VIEWS.includes(a.page)) go.onOpen(a.page as View);
      return;
    case "settings":
      return go.onSettings(a.section as Section);
    case "runPlaybook":
      return go.onSettings("playbooks");
    case "review":
      return go.onOpen("workboard");
    case "toggle":
    case "turnOn":
      return go.onSettings(FEATURE_SECTION[a.feature] ?? "general");
    case "openUrl":
      if (remote) window.open(a.url, "_blank", "noopener");
      else void openUrl(a.url).catch(() => {});
      return;
    case "play":
      if (!remote) void libraryPlay(a.appId).catch(() => {});
      return;
    case "reroll":
    case "retry":
      return reload();
  }
}

/** Any module in the shared tile model, cut to what its size holds. */
function ViewBody({
  view,
  size,
  go,
  remote,
  reload,
}: {
  view: TileView;
  size: SlotSize;
  go: HomeRoutes;
  remote: boolean;
  reload: () => void;
}) {
  const fit = FIT[size];
  const run = (a: TileAction) => runAction(a, go, remote, reload);
  const rows = view.rows.slice(0, fit.rows);
  const buttons = view.buttons.slice(0, fit.buttons);
  const head = (
    <>
      <span className={`home-big tone-${view.text.tone}`}>{view.text.big}</span>
      <span className="home-muted">{view.text.caption}</span>
      {size !== "small" && view.text.detail && <span>{view.text.detail}</span>}
      {size !== "small" && view.progress !== null && (
        <span className="xp-track" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={view.progress}>
          <span className="xp-fill" style={{ width: `${view.progress}%` }} />
        </span>
      )}
    </>
  );
  // A Small tile is one tap; so is any tile whose state wants its one button.
  const tap = view.tap ?? (size === "small" ? (view.buttons[0]?.action ?? null) : null);
  return (
    <div className={`home-stack state-${view.state}`}>
      {tap ? (
        <Go label={view.text.big} onClick={() => run(tap)}>
          {head}
        </Go>
      ) : (
        head
      )}
      {rows.length > 0 && (
        <ul className="home-rows home-list">
          {rows.map((r, i) => (
            <li key={`${r.label}-${i}`}>
              {r.tap ? (
                <button type="button" className="home-row home-pair" onClick={() => run(r.tap!)}>
                  <span>{r.label}</span>
                  <span className={`tone-${r.tone}`}>{r.value}</span>
                </button>
              ) : (
                <span className="home-pair">
                  <span>{r.label}</span>
                  <span className={`tone-${r.tone}`}>{r.value}</span>
                </span>
              )}
            </li>
          ))}
        </ul>
      )}
      {buttons.length > 0 && (
        <div className="home-buttons">
          {buttons.map((b) => (
            <button key={b.label} type="button" className={b.primary ? undefined : "quiet"} onClick={() => run(b.action)}>
              {b.label}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

/** A tile this device can't read draws quieter. */
function away(tile: HomeTile): boolean {
  return tile.kind === "view" && tile.view.state === "unsupported";
}

/** A tile's body, a button where the tile goes somewhere. */
function Go({ label, onClick, children }: { label: string; onClick: () => void; children: ReactNode }) {
  return (
    <button type="button" className="home-go" aria-label={label} onClick={onClick}>
      {children}
    </button>
  );
}

function body(tile: HomeTile, slot: HomeSlot, go: HomeRoutes, remote: boolean, reload: () => void): ReactNode {
  switch (tile.kind) {
    case "objectives": {
      const steps = tile.mission?.steps.filter((s) => s.status !== "cancelled") ?? [];
      return (
        <Go label="Open Objectives" onClick={() => go.onOpen("objectives")}>
          {tile.mission && steps.length > 0 ? (
            <>
              <span className="home-count">
                {tile.mission.done} / {tile.mission.total}
              </span>
              {tile.mission.title && <span className="home-strong">{tile.mission.title}</span>}
              <span className="home-steps">
                {steps.slice(0, slot.size === "small" ? 2 : 4).map((s) => (
                  <span key={s.id} className={s.status === "verified" ? "done" : ""}>
                    {s.text}
                  </span>
                ))}
              </span>
            </>
          ) : (
            <span className="home-muted">{NO_STEPS}</span>
          )}
          <XpBar xp={tile.xp} />
        </Go>
      );
    }
    case "streak":
      return (
        <Go label="Open Objectives" onClick={() => go.onOpen("objectives")}>
          <XpBar xp={tile.xp} />
          <Streak days={tile.xp.streak} />
        </Go>
      );
    case "recentChats":
      return tile.chats.length === 0 ? (
        <span className="home-muted">No chats yet.</span>
      ) : (
        <ul className="home-rows">
          {tile.chats.map((c) => (
            <li key={c.id}>
              <button type="button" className="home-row" onClick={() => go.onOpenChat(c.id)}>
                {c.title || "Untitled chat"}
              </button>
            </li>
          ))}
        </ul>
      );
    case "automations":
      return (
        <Go label="Open Automations" onClick={() => go.onOpen("automations")}>
          <span className="home-count">{tile.active} on</span>
          {tile.next.length === 0 ? (
            <span className="home-muted">Nothing scheduled.</span>
          ) : (
            <span className="home-rows">
              {tile.next.map((a) => (
                <span key={a.id}>
                  <span className="home-strong">{a.name}</span>
                  <span className="home-muted">{a.summary}</span>
                </span>
              ))}
            </span>
          )}
        </Go>
      );
    case "pulse": {
      const idea = tile.idea;
      return (
        <div className="home-stack">
          {tile.post ? (
            <Go label="Open Pulse" onClick={() => go.onOpen("pulse")}>
              <span className="home-strong">{tile.post.title}</span>
              <span className="home-muted">{tile.post.ago}</span>
            </Go>
          ) : (
            <span className="home-muted">All quiet. New finds land here.</span>
          )}
          {idea && (
            <button
              type="button"
              className="home-row"
              onClick={() => {
                // Taking the idea is its accept, as in Pulse; the box still waits on Send.
                void pulseAct(idea.id, "accept").catch(() => {});
                go.onAsk(idea.action);
              }}
            >
              {idea.title}
            </button>
          )}
        </div>
      );
    }
    case "personality": {
      const url = tile.picture ? artUrl(tile.picture) : null;
      const inner = (
        <>
          {url && <img className="home-nyx" src={url} alt="" draggable={false} />}
          <span className="home-strong">{tile.name || "Default voice"}</span>
        </>
      );
      return remote ? (
        <div className="home-stack">{inner}</div>
      ) : (
        <Go label="Open Personality settings" onClick={() => go.onSettings("personality")}>
          {inner}
        </Go>
      );
    }
    case "upNext":
      return (
        <Go label="Open Library" onClick={() => go.onOpen("library")}>
          {tile.games.length === 0 ? (
            <span className="home-muted">No Steam games found.</span>
          ) : (
            <span className="home-rows">
              {tile.games.map((g) => (
                <span key={g.appId}>{g.name}</span>
              ))}
            </span>
          )}
        </Go>
      );
    case "error":
      return <span className="home-muted">{tile.message}</span>;
    case "view":
      return <ViewBody view={tile.view} size={slot.size} go={go} remote={remote} reload={reload} />;
  }
}

/** Home: the cabin's tile grid from the same home_layout.json, every tile
 * filled with what this computer knows. Read only: Edit Home stays in the
 * cabin. */
export function Home({
  conversationId,
  remote,
  ...go
}: HomeRoutes & { conversationId?: string; remote: boolean }) {
  const [page, setPage] = useState<HomePage | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const row = useChipRow({ surface: "home" });

  function tapChip(chip: QuickChip) {
    row.picked(chip);
    if (chip.action === "go" && chip.view) go.onOpen(chip.view as View);
    else go.onAsk(chip.value);
  }

  const [reads, setReads] = useState(0);
  const reload = () => setReads((n) => n + 1);

  useEffect(() => {
    let live = true;
    getHome(conversationId).then(
      (p) => {
        if (!live) return;
        setPage(p);
        setNote(null);
      },
      (err) => live && setNote(String(err)),
    );
    // A slow answer for an older read never overwrites a newer one.
    return () => {
      live = false;
    };
  }, [conversationId, reads]);

  return (
    <section className="home" aria-label="Home">
      <div className="home-head">
        <h2>Home</h2>
        {page?.xpBar && <XpBar xp={page.xp} full />}
      </div>
      <ChipRow chips={row.chips} onTap={tapChip} onHide={row.hide} />
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      {page === null && !note && <p className="empty">Loading Home…</p>}
      {page && (
        <>
          <ul className="home-grid" aria-label="Tiles">
            {page.slots.map((slot) => (
              <li
                key={slot.slot}
                className={`home-tile ${slot.size}${away(slot.tile) ? " away" : ""}`}
                aria-label={slot.name}
              >
                <h3>
                  {slot.name}
                  {slot.tile.kind === "view" && slot.tile.view.tag && <span className="home-tag">{slot.tile.view.tag}</span>}
                </h3>
                {body(slot.tile, slot, go, remote, reload)}
              </li>
            ))}
          </ul>
          <div className="home-glance" role="group" aria-label="At a glance">
            <button type="button" onClick={() => go.onOpen("memory")}>
              Memory · {page.glance.memories ?? "—"}
              {page.glance.proposed > 0 && ` (${page.glance.proposed} to review)`}
            </button>
            <button type="button" onClick={() => go.onOpen("connectors")}>
              Connectors · {page.glance.connected} / {page.glance.connectors} connected
              {page.glance.failing > 0 && ` · ${page.glance.failing} failing`}
            </button>
          </div>
          <p className="home-muted home-foot">
            {PRESETS[page.preset] ?? page.preset} layout. {EDIT_NOTE}
          </p>
        </>
      )}
    </section>
  );
}
