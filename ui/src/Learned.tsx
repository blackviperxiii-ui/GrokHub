import { useEffect, useState } from "react";
import { getLearned, learnedAct, type LearnedAct, type LearnedRow, type LearnedStage, type LearnedTrust, type LearnedView, type TrustDial, type TrustGoal, type TrustRow } from "./api";
import { PRODUCT_NAME } from "./brand";

export const LEARNED_NOTE = `What ${PRODUCT_NAME} worked out from how you use it: which quick chips and cards you take, and how runs go. It stays on this computer and holds counts only, never what you typed. Forget clears a line so it starts fresh. That's wrong stops a line from being counted or used again. Always makes a line your own rule, whatever is counted.`;
export const LEARNED_PAUSED = "Learning is paused. Nothing new is counted until you turn it back on.";
export const LEARNED_EMPTY = "Nothing learned yet. It builds up as you use quick chips, cards and runs.";

export const TRUST_NOTE = `How far ${PRODUCT_NAME} may go in each area. Watch only keeps learning and shows nothing. Offer only asks every time. Act when sure makes a change once you've said yes to it three times, and it can always be undone. Deleting, spending, sending and permissions always ask.`;
export const TRUST_ASKING = "Too much was undone lately, so everything asks first for a few days.";
export const TRUST_RESTING = "An area that got worse asks first for a month.";
export const DECISIONS_EMPTY = "No decisions yet. They show up here once there is something it could change for you.";

const DIAL: Record<TrustDial, string> = {
  watch: "Watch only",
  offer: "Offer only",
  act: "Act when sure",
};

const GOAL: Record<TrustGoal, string> = {
  cheapest: "Lowest cost",
  fastest: "Fastest answers",
  quiet: "Fewest interruptions",
  best_quality: "Best quality",
};
export const GOAL_NOTE = "When two changes are both likely right, the one that serves this goes first.";

/** Why changes wait right now, or nothing when they don't. */
export function waitingLine(now: LearnedTrust["now"]) {
  if (now.halted) return "Halt is on, so changes wait.";
  if (now.gaming) return "A game is running, so changes wait.";
  if (now.quiet) return "It's quiet hours, so changes wait.";
  if (now.busy) return "You're in the middle of something, so changes wait.";
  return null;
}

/** One area's scoreboard line, or nothing before anything was counted. */
export function scoreLine(row: TrustRow) {
  if (row.went === null) return null;
  const went = `${row.went}% of what it offered or changed here went your way in the last four weeks`;
  const rest = row.resting ? " It asks first here for a month." : "";
  if (row.trend === "better") return `${went}, up from ${row.before}%.`;
  if (row.trend === "worse") return `${went}, down from ${row.before}%.${rest}`;
  return `${went}.${rest}`;
}

function leftToday(n: number) {
  if (n === 0) return "It has made all its changes for today, so it asks for the rest.";
  return n === 1 ? "It may make 1 more change on its own today." : `It may make ${n} more changes on its own today.`;
}

/** The decision engine's dials and its newest decisions. The cabin's
 * `/learned decisions` and `/learned trust` show and set the same. */
function Trust({ trust, busy, run }: { trust: LearnedTrust; busy: boolean; run: (act: LearnedAct, done: string) => Promise<void> }) {
  return (
    <>
      <h3>How far it may go</h3>
      <p className="note">{TRUST_NOTE}</p>
      {(trust.askingFirst || trust.dials.some((d) => d.resting)) && (
        <div className="confirm">
          <p className="note warn">{trust.askingFirst ? TRUST_ASKING : TRUST_RESTING}</p>
          <button type="button" className="quiet" disabled={busy} onClick={() => void run({ kind: "carryOn" }, "It may act on its own again.")}>
            Carry on
          </button>
        </div>
      )}
      {trust.dials.map((d) => (
        <label key={d.area}>
          {d.label}
          <select value={d.dial} disabled={busy} onChange={(e) => void run({ kind: "trust", area: d.area, dial: e.target.value as TrustDial }, `${d.label} is set to ${DIAL[e.target.value as TrustDial].toLowerCase()}.`)}>
            {(Object.keys(DIAL) as TrustDial[]).map((dial) => (
              <option key={dial} value={dial}>
                {DIAL[dial]}
              </option>
            ))}
          </select>
        </label>
      ))}
      {trust.dials.map((d) => {
        const line = scoreLine(d);
        return (
          line && (
            <p key={d.area} className="note">
              {d.label}: {line}
            </p>
          )
        );
      })}
      <p className="note">{leftToday(trust.leftToday)}</p>
      {waitingLine(trust.now) && <p className="note">{waitingLine(trust.now)}</p>}
      <label>
        What matters most
        <select
          value={trust.goals[0]?.goal ?? ""}
          disabled={busy}
          onChange={(e) => {
            const first = e.target.value as TrustGoal | "";
            const rest = trust.goals.map((g) => g.goal).filter((g) => g !== first);
            void run({ kind: "goals", goals: first === "" ? [] : [first, ...rest] }, first === "" ? "No goal set." : `${GOAL[first]} comes first now.`);
          }}
        >
          <option value="">Nothing set</option>
          {(Object.keys(GOAL) as TrustGoal[]).map((goal) => (
            <option key={goal} value={goal}>
              {GOAL[goal]}
            </option>
          ))}
        </select>
      </label>
      <p className="note">{GOAL_NOTE}</p>
      <h3>What it decided</h3>
      {trust.decisions.length === 0 && <p className="empty">{DECISIONS_EMPTY}</p>}
      {trust.decisions.length > 0 && (
        <ul className="rows" aria-label="Decisions">
          {trust.decisions.map((d, i) => (
            <li key={`${d.atMs}-${i}`} className="card" aria-label={`${d.did}: ${d.title}`}>
              <div className="card-title">
                {d.did}: {d.title}
              </div>
              <div className="card-status">{d.why}</div>
              <div className="card-status">{new Date(d.atMs).toLocaleString()}</div>
            </li>
          ))}
        </ul>
      )}
    </>
  );
}

const STAGE: Record<LearnedStage, string> = {
  watching: "Still watching",
  suggesting: "Sure enough to suggest",
  acting: "Sure enough to act",
  measuring: "A measurement",
  wrong: "Marked wrong",
  pinned: "You said always",
};

function proof(row: LearnedRow) {
  if (row.stage === "wrong" || row.stage === "pinned") return STAGE[row.stage];
  const seen = row.evidence === 1 ? "Seen once" : `Seen ${row.evidence} times`;
  return `${seen} · ${row.sure}% sure · ${STAGE[row.stage]}`;
}

/** Settings → Privacy & data: every belief the learning engine holds, with
 * its evidence, and the buttons that overrule it. The cabin's `/learned`
 * prints the same lines. */
export function Learned() {
  const [view, setView] = useState<LearnedView | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState(false);

  useEffect(() => {
    getLearned().then(setView, (err) => setNote(String(err)));
  }, []);

  async function run(act: LearnedAct, done: string) {
    setBusy(true);
    setConfirming(false);
    try {
      setView(await learnedAct(act));
      setNote(done);
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  const empty = view !== null && view.rows.length === 0 && view.singleChips === 0;

  return (
    <div className="interests" role="group" aria-label="What the app learned">
      <h2>What {PRODUCT_NAME} learned</h2>
      <p className="note">{LEARNED_NOTE}</p>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      {view?.paused && <p className="note warn">{LEARNED_PAUSED}</p>}
      {view && (
        <div className="card-buttons">
          <button type="button" className="quiet" disabled={busy} onClick={() => void run({ kind: view.paused ? "resume" : "pause" }, view.paused ? "Learning is back on." : "Learning is paused.")}>
            {view.paused ? "Resume learning" : "Pause learning"}
          </button>
        </div>
      )}
      {view === null && !note && <p className="empty">Loading…</p>}
      {empty && <p className="empty">{LEARNED_EMPTY}</p>}
      {view && view.rows.length > 0 && (
        <ul className="rows" aria-label="Learned">
          {view.rows.map((row) => (
            <li key={row.key} className={row.stage === "wrong" ? "card muted" : "card"} aria-label={row.title}>
              <div className="card-title">{row.title}</div>
              <div className="card-status">{row.says}</div>
              <div className="card-status">{proof(row)}</div>
              {row.stage !== "wrong" && (
                <div className="card-buttons">
                  <button type="button" className="quiet" disabled={busy} onClick={() => void run({ kind: "forget", key: row.key }, `Forgot ${row.title}.`)}>
                    Forget
                  </button>
                  <button type="button" className="quiet" disabled={busy} onClick={() => void run({ kind: "wrong", key: row.key }, `${row.title} won't be counted or used again.`)}>
                    That's wrong
                  </button>
                  {row.stage !== "pinned" && row.stage !== "measuring" && (
                    <button type="button" className="quiet" disabled={busy} onClick={() => void run({ kind: "always", key: row.key }, `${row.title} is your rule now.`)}>
                      Always
                    </button>
                  )}
                </div>
              )}
            </li>
          ))}
        </ul>
      )}
      {view && view.singleChips > 0 && (
        <p className="note">
          Plus counts for {view.singleChips === 1 ? "1 single quick chip" : `${view.singleChips} single quick chips`}, which rank the chip rows.
        </p>
      )}
      {view && <Trust trust={view.trust} busy={busy} run={run} />}
      {view && !empty && !confirming && (
        <div className="card-buttons">
          <button type="button" className="quiet" disabled={busy} onClick={() => setConfirming(true)}>
            Forget everything…
          </button>
        </div>
      )}
      {confirming && (
        <div className="confirm">
          <p className="note warn">This clears everything {PRODUCT_NAME} learned from how you use it. It can't be undone.</p>
          <button type="button" disabled={busy} onClick={() => void run({ kind: "forgetAll" }, "Forgot everything.")}>
            Forget everything
          </button>
          <button type="button" className="quiet" onClick={() => setConfirming(false)}>
            Cancel
          </button>
        </div>
      )}
    </div>
  );
}
