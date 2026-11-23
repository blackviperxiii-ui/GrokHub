// Chat's right column on wide windows (slides 02 and 20): under the docked
// companion, the open mission's Objectives, the PC loadout and the newest
// Pulse posts. Each card is a glance with a way to its page; a card whose
// data doesn't come (no mission yet, Pulse off, a desktop that refuses) says
// so or stays out. The PC card reads this PC's tile on its own, whether or not
// Home shows it; a paired browser reads it from the host's Home instead.
// Amber stays on the chat's own Send.
import { useEffect, useState } from "react";
import {
  getHome,
  getMission,
  getPcSnapshot,
  getPulse,
  listChats,
  type ChatRow,
  type MissionPage,
  type PulsePage,
  type TileView,
} from "./api";
import { isCodeSession } from "./CodeProjects";
import { NO_STEPS, XpBar } from "./Home";
import { NyxDeskControls } from "./NyxDock";
import type { View } from "./Palette";

/** How often the cards read again while Chat shows, in ms. */
const POLL_MS = 30_000;

/** Reads `load` while `active`, then every POLL_MS; an error leaves null. */
function usePoll<T>(active: boolean, load: () => Promise<T>, key: string): T | null {
  const [value, setValue] = useState<T | null>(null);
  useEffect(() => {
    if (!active) return;
    let live = true;
    const read = () =>
      Promise.resolve()
        .then(load)
        .then(
          (next) => {
            if (live) setValue(next);
          },
          () => {
            if (live) setValue(null);
          },
        );
    void read();
    const timer = window.setInterval(() => void read(), POLL_MS);
    return () => {
      live = false;
      window.clearInterval(timer);
    };
    // `load` is rebuilt every render; `key` says when what it reads changed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, key]);
  return value;
}

function Objectives({ page, onOpen }: { page: MissionPage; onOpen: () => void }) {
  const mission = page.mission;
  const steps = mission?.steps.filter((step) => step.status !== "cancelled").slice(0, 3) ?? [];
  return (
    <section className="card side-glance" aria-label="Objectives">
      <div className="side-head">
        <h3>Objectives</h3>
        {mission && mission.total > 0 && (
          <span className="side-count">
            {mission.done} / {mission.total}
          </span>
        )}
      </div>
      {mission && steps.length > 0 ? (
        <>
          {mission.title && <p className="side-strong">{mission.title}</p>}
          <ul className="side-steps">
            {steps.map((step) => (
              <li key={step.id} className={step.status === "verified" ? "done" : undefined}>
                {step.text}
              </li>
            ))}
          </ul>
        </>
      ) : (
        <p className="home-muted">{NO_STEPS}</p>
      )}
      <XpBar xp={page.xp} />
      <button type="button" className="quiet" onClick={onOpen}>
        Open Objectives
      </button>
    </section>
  );
}

function PcCard({ view, onOpen }: { view: TileView; onOpen: () => void }) {
  return (
    <section className="card side-glance" aria-label="PC loadout">
      <div className="side-head">
        <h3>PC loadout</h3>
        <span className={`side-count tone-${view.text.tone}`}>{view.text.big}</span>
      </div>
      {view.text.caption && <p className="home-muted">{view.text.caption}</p>}
      {view.rows.length > 0 && (
        <ul className="side-pairs">
          {view.rows.slice(0, 2).map((row, i) => (
            <li key={`${row.label}-${i}`}>
              <span className="home-muted">{row.label}</span>
              <span className={`tone-${row.tone}`}>{row.value}</span>
            </li>
          ))}
        </ul>
      )}
      <button type="button" className="quiet" onClick={onOpen}>
        Open Home
      </button>
    </section>
  );
}

function FromPulse({ page, onOpen }: { page: PulsePage; onOpen: () => void }) {
  const posts = page.feed.slice(0, 2);
  return (
    <section className="card side-glance" aria-label="From Pulse">
      <div className="side-head">
        <h3>From Pulse</h3>
        {page.today > 0 && <span className="side-count">{page.today} new</span>}
      </div>
      {posts.length > 0 ? (
        <ul className="side-posts">
          {posts.map((post) => (
            <li key={post.id}>
              <span className="side-strong">{post.title}</span>
              <span className="home-muted">{post.ago}</span>
            </li>
          ))}
        </ul>
      ) : (
        <p className="home-muted">{page.note}</p>
      )}
      <button type="button" className="quiet" onClick={onOpen}>
        Open Pulse
      </button>
    </section>
  );
}

export function ChatSide({
  active,
  conversationId,
  pulse,
  remote = false,
  onOpen,
}: {
  /** Chat shows: the cards read only then. */
  active: boolean;
  /** The chat open in Chat; its mission is the one shown. */
  conversationId?: string;
  /** This app has Pulse. */
  pulse: boolean;
  /** A paired browser: the PC tile comes from the host's Home. */
  remote?: boolean;
  onOpen: (view: View) => void;
}) {
  const id = conversationId ?? "";
  const mission = usePoll(active, () => getMission(conversationId), `mission:${id}`);
  const snapshot = usePoll(active && !remote, () => getPcSnapshot(), "pc");
  const home = usePoll(active && remote, () => getHome(conversationId), `home:${id}`);
  const feed = usePoll(active && pulse, () => getPulse(), "pulse");
  const slot = home?.slots.find((s) => s.module === "pc_loadout")?.tile;
  const pc = remote ? (slot?.kind === "view" ? slot.view : null) : snapshot;
  return (
    <>
      {mission && <Objectives page={mission} onOpen={() => onOpen("objectives")} />}
      {pc && pc.state !== "unsupported" && <PcCard view={pc} onOpen={() => onOpen("home")} />}
      {pulse && feed && <FromPulse page={feed} onOpen={() => onOpen("pulse")} />}
    </>
  );
}

/** Recent chats beside the chat: at most this many. */
export const RECENT_MAX = 5;
export const RECENT_EMPTY = "Other chats show up here.";

/** The chats to list beside `openId`: newest first, without it, Main, or
 * Code project sessions (those stay in the Editor Projects tab). */
export function recentChats(rows: ChatRow[], openId: string | undefined): ChatRow[] {
  return rows
    .filter((row) => row.id !== openId && !row.main && !isCodeSession(row))
    .sort((a, b) => b.updated - a.updated)
    .slice(0, RECENT_MAX);
}

/** Objectives beside chat, the left column: the open chat's objective with
 * its current step marked, then recent chats to switch to. Desktop only:
 * a paired browser has no chat list. */
export function ChatLeft({
  active,
  conversationId,
  onOpen,
  onOpenChat,
}: {
  active: boolean;
  conversationId?: string;
  onOpen: (view: View) => void;
  onOpenChat: (id: string) => void;
}) {
  const id = conversationId ?? "";
  const page = usePoll(active, () => getMission(conversationId), `mission:${id}`);
  const chats = usePoll(active, () => listChats(), `chats:${id}`);
  const mission = page?.mission ?? null;
  const steps = mission?.steps.filter((step) => step.status !== "cancelled") ?? [];
  const current = steps.find((step) => step.status !== "verified")?.id;
  const recent = chats ? recentChats(chats, conversationId) : [];
  return (
    <aside className="chat-left" aria-label="Objectives">
      <section className="card side-glance" aria-label="Steps">
        <div className="side-head">
          <h3>Objectives</h3>
          {mission && mission.total > 0 && (
            <span className="side-count">
              {mission.done} / {mission.total}
            </span>
          )}
        </div>
        {mission && steps.length > 0 ? (
          <>
            {mission.title && <p className="side-strong">{mission.title}</p>}
            <ol className="side-steps">
              {steps.map((step) => (
                <li
                  key={step.id}
                  className={step.status === "verified" ? "done" : step.id === current ? "current" : undefined}
                  aria-current={step.id === current ? "step" : undefined}
                >
                  {step.text}
                </li>
              ))}
            </ol>
          </>
        ) : (
          <p className="home-muted">{NO_STEPS}</p>
        )}
        <button type="button" className="quiet" onClick={() => onOpen("objectives")}>
          Open Objectives
        </button>
      </section>
      <section className="card side-glance" aria-label="Recent chats">
        <div className="side-head">
          <h3>Recent</h3>
        </div>
        {recent.length > 0 ? (
          <ul className="home-rows">
            {recent.map((row) => (
              <li key={row.id}>
                <button type="button" className="home-row" onClick={() => onOpenChat(row.id)}>
                  {row.title.trim() || "Untitled chat"}
                </button>
              </li>
            ))}
          </ul>
        ) : (
          <p className="home-muted">{RECENT_EMPTY}</p>
        )}
      </section>
      <NyxDeskControls framed active={active} />
    </aside>
  );
}

/** Objectives beside chat, the right column: three readings from this PC's
 * own PC Loadout tile. Phones have none, so it stays out there. */
export function ChatSnapshot({ active, onOpen }: { active: boolean; onOpen: (view: View) => void }) {
  const view = usePoll(active, () => getPcSnapshot(), "pc");
  if (!view || view.state === "unsupported") return null;
  const tiles = [
    { label: view.text.caption || "This PC", value: view.text.big, tone: view.text.tone },
    ...view.rows.slice(0, 2),
  ];
  return (
    <aside className="chat-snapshot" aria-label="Snapshot">
      <section className="card side-glance" aria-label="PC snapshot">
        <div className="side-head">
          <h3>Snapshot</h3>
        </div>
        <ul className="snapshot-tiles">
          {tiles.map((tile, i) => (
            <li key={`${tile.label}-${i}`} className={i === 0 ? "snapshot-tile lead" : "snapshot-tile"}>
              <span className={`snapshot-value tone-${tile.tone}`}>{tile.value}</span>
              <span className="home-muted">{tile.label}</span>
            </li>
          ))}
        </ul>
        <button type="button" className="quiet" onClick={() => onOpen("home")}>
          Open Home
        </button>
      </section>
    </aside>
  );
}
