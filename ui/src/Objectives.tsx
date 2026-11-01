import { useEffect, useState } from "react";
import { getMission, setMissionDone, type MissionPage, type StepStatus } from "./api";
import { NO_STEPS, Streak, XpBar } from "./Home";

/** The tag at the end of a step's row, as the cabin's Objectives writes it. */
export const STEP_TAGS: Record<StepStatus, string> = {
  verified: "DONE",
  attempted: "IN PROGRESS",
  unverified: "CHECK",
  planned: "",
  cancelled: "",
};

const STATUS: Record<string, string> = {
  active: "In progress",
  needs_approval: "Waiting on you",
  paused: "Paused",
  blocked: "Needs a look",
  done: "Done",
};

export const ACCEPT_HELP = "Accept as done marks the finished steps checked and counts the task for XP.";

/** `WEDNESDAY · OCT 28`, the cabin's caption over the title. */
export function dayCaption(now: Date = new Date()): string {
  const day = now.toLocaleDateString("en-US", { weekday: "long" }).toUpperCase();
  const month = now.toLocaleDateString("en-US", { month: "short" }).toUpperCase();
  return `${day} · ${month} ${now.getDate()}`;
}

/** Objectives: the open chat's checklist with level, XP and the weekly
 * streak, the cabin's Objectives page. The steps are the chat's own to-dos;
 * on the desktop, Accept as done (and Undo) writes the cabin's outcome log. */
export function Objectives({
  conversationId,
  remote,
  onOpenChat,
  onHome,
}: {
  conversationId?: string;
  remote: boolean;
  onOpenChat: (id: string) => void;
  onHome: () => void;
}) {
  const [page, setPage] = useState<MissionPage | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getMission(conversationId).then(setPage, (err) => setNote(String(err)));
  }, [conversationId]);

  async function toggle(done: boolean) {
    setBusy(true);
    try {
      setPage(await setMissionDone(done, conversationId));
      setNote(done ? "Accepted. The steps count as done." : "Undone. The task no longer counts.");
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  const mission = page?.mission ?? null;
  const steps = mission?.steps.filter((s) => s.status !== "cancelled") ?? [];
  const current = steps.findIndex((s) => s.status !== "verified");

  return (
    <section className="objectives" aria-label="Objectives">
      <div className="objectives-head">
        <div>
          <p className="home-caption">{dayCaption()}</p>
          <h2>Objectives</h2>
        </div>
        {page && <XpBar xp={page.xp} full />}
      </div>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      {page === null && !note && <p className="empty">Loading objectives…</p>}
      {page && (
        <div className="card objectives-card">
          <div className="xp-line">
            <span className="xp-level">Today's objectives</span>
            {mission && mission.total > 0 && (
              <span className="xp-amount">
                {mission.done} / {mission.total}
              </span>
            )}
          </div>
          {mission && steps.length > 0 ? (
            <>
              <p className="home-muted">
                {mission.title && <strong>{mission.title}</strong>} {STATUS[mission.status] ?? mission.status}
              </p>
              <ol className="objectives-steps" aria-label="Steps">
                {steps.map((s, i) => (
                  <li key={s.id} className={`${s.status}${i === current ? " current" : ""}`}>
                    <span className="objectives-text">{s.text}</span>
                    {STEP_TAGS[s.status] && <span className="objectives-tag">{STEP_TAGS[s.status]}</span>}
                  </li>
                ))}
              </ol>
              <div className="card-buttons">
                <button type="button" className="quiet" onClick={() => onOpenChat(mission.conversationId)}>
                  Open the chat
                </button>
                {!remote && mission.canAccept && (
                  <button type="button" disabled={busy} title={ACCEPT_HELP} onClick={() => void toggle(true)}>
                    Accept as done
                  </button>
                )}
                {!remote && mission.accepted && (
                  <button type="button" className="quiet" disabled={busy} onClick={() => void toggle(false)}>
                    Undo
                  </button>
                )}
              </div>
            </>
          ) : (
            <p className="home-muted">{NO_STEPS}</p>
          )}
          <Streak days={page.xp.streak} />
          {page.xp.today > 0 && <p className="home-muted">+{page.xp.today.toLocaleString("en-US")} XP today</p>}
        </div>
      )}
      <button type="button" className="quiet objectives-back" onClick={onHome}>
        Back to Home
      </button>
    </section>
  );
}
