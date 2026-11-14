// Chat's right column on wide windows (slides 02 and 20): under the docked
// companion, the open mission's Objectives, the PC loadout and the newest
// Pulse posts. Each card is a glance with a way to its page; a card whose
// data doesn't come (no mission yet, the tile off Home, Pulse off, a desktop
// that refuses) says so or stays out. Amber stays on the chat's own Send.
import { useEffect, useState } from "react";
import { getHome, getMission, getPulse, type MissionPage, type PulsePage, type TileView } from "./api";
import { NO_STEPS, XpBar } from "./Home";
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
  onOpen,
}: {
  /** Chat shows: the cards read only then. */
  active: boolean;
  /** The chat open in Chat; its mission is the one shown. */
  conversationId?: string;
  /** This app has Pulse. */
  pulse: boolean;
  onOpen: (view: View) => void;
}) {
  const id = conversationId ?? "";
  const mission = usePoll(active, () => getMission(conversationId), `mission:${id}`);
  const home = usePoll(active, () => getHome(conversationId), `home:${id}`);
  const feed = usePoll(active && pulse, () => getPulse(), "pulse");
  const pc = home?.slots.find((slot) => slot.module === "pc_loadout" && slot.tile.kind === "view")?.tile;
  return (
    <>
      {mission && <Objectives page={mission} onOpen={() => onOpen("objectives")} />}
      {pc?.kind === "view" && <PcCard view={pc.view} onOpen={() => onOpen("home")} />}
      {pulse && feed && <FromPulse page={feed} onOpen={() => onOpen("pulse")} />}
    </>
  );
}
