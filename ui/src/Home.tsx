import { useEffect, useState, type ReactNode } from "react";
import { getHome, type HomePage, type HomeSlot, type HomeTile, type Xp } from "./api";
import { COMPANION_NAME } from "./brand";
import { artUrl } from "./nyx";
import type { View } from "./Palette";
import type { Section } from "./sections";

export const CABIN_NOTE = "Open in the cabin";
export const DESKTOP_NOTE = "Open on your computer";
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

/** A tile's body, a button where the tile goes somewhere. */
function Go({ label, onClick, children }: { label: string; onClick: () => void; children: ReactNode }) {
  return (
    <button type="button" className="home-go" aria-label={label} onClick={onClick}>
      {children}
    </button>
  );
}

function body(tile: HomeTile, slot: HomeSlot, go: HomeRoutes, remote: boolean): ReactNode {
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
            <button type="button" className="home-row" onClick={() => go.onAsk(idea.action)}>
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
    case "desktop":
      return <span className="home-muted">{DESKTOP_NOTE}</span>;
    case "cabin":
      return (
        <>
          <span className="home-muted">{slot.purpose}</span>
          <span className="home-cabin">{CABIN_NOTE}</span>
        </>
      );
  }
}

/** Home: the cabin's tile grid from the same home_layout.json, showing what
 * this app knows. Tiles only the cabin can fill say so. Read only: Edit
 * Home stays in the cabin. */
export function Home({
  conversationId,
  remote,
  ...go
}: HomeRoutes & { conversationId?: string; remote: boolean }) {
  const [page, setPage] = useState<HomePage | null>(null);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    getHome(conversationId).then(
      (p) => {
        setPage(p);
        setNote(null);
      },
      (err) => setNote(String(err)),
    );
  }, [conversationId]);

  return (
    <section className="home" aria-label="Home">
      <div className="home-head">
        <h2>Home</h2>
        {page?.xpBar && <XpBar xp={page.xp} full />}
      </div>
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
                className={`home-tile ${slot.size}${slot.tile.kind === "cabin" || slot.tile.kind === "desktop" ? " away" : ""}`}
                aria-label={slot.name}
              >
                <h3>{slot.name}</h3>
                {body(slot.tile, slot, go, remote)}
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
