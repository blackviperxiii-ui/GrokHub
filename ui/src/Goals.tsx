import { useEffect, useState, type FormEvent } from "react";
import { getGoals, goalAct, type FollowUp, type GoalAct, type GoalBoard, type GoalCard, type YourWeek } from "./api";

export const ADD_HINT = "What do you want to do?";
export const TODAY_EMPTY = "Nothing for today yet. Add a goal below and it shows up here.";
export const MINE_EMPTY = 'No goals yet. Type one above, like "99 Fishing in OSRS" or "finish Elden Ring".';
export const SHELF_EMPTY = "Goals you finish land here.";

function Bar({ frac, label }: { frac: number; label: string }) {
  const pct = Math.round(Math.min(1, Math.max(0, frac)) * 100);
  return (
    <span className="xp-track" role="progressbar" aria-label={label} aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct}>
      <span className="xp-fill" style={{ width: `${pct}%` }} />
    </span>
  );
}

/** A goal's name, where it stands and its bar. */
function Summary({ goal }: { goal: GoalCard }) {
  return (
    <div className="goal-summary">
      <strong>{goal.title}</strong>
      <span className="home-muted">{goal.detail}</span>
      <Bar frac={goal.frac} label={goal.title} />
    </div>
  );
}

export const ASK_TRACKED = "Tracking it. It's on Objectives under My goals.";

/** Under a reply that noticed a game goal: Track puts it on My goals, Not
 * now hides it for 30 days. Left alone, it waits in the suggested tray. */
export function GoalAsk({ id, title, onDone }: { id: string; title: string; onDone: () => void }) {
  const [busy, setBusy] = useState(false);
  const [said, setSaid] = useState<string | null>(null);

  async function press(act: "track" | "notNow") {
    setBusy(true);
    try {
      await goalAct([{ act, id }]);
      if (act === "track") setSaid(ASK_TRACKED);
      else onDone();
    } catch (err) {
      setSaid(String(err));
    } finally {
      setBusy(false);
    }
  }

  if (said) {
    return (
      <p className="goal-ask home-muted" role="status">
        {said}
      </p>
    );
  }
  return (
    <div className="goal-ask" role="group" aria-label="Track this goal?">
      <span>
        Track this goal? <strong>{title}</strong>
      </span>
      <button type="button" disabled={busy} onClick={() => void press("track")}>
        Track
      </button>
      <button type="button" className="quiet" disabled={busy} onClick={() => void press("notNow")}>
        Not now
      </button>
    </div>
  );
}

/** "N goals done this season". */
export function doneLine(n: number): string {
  return `${n} ${n === 1 ? "goal" : "goals"} done this season`;
}

/** Objectives' goal cards, the same as the cabin's: Today (the goal closest
 * to done and one left a week), My goals (type a goal, +1, Done, Drop and
 * the suggested tray) and This season (goals done and the trophy shelf).
 * `onChange` runs after a press so the XP bar can catch up; `onAsk` puts a
 * follow-up's ask in the chat box. */
/** The button under a follow-up: puts its ask in the chat box. */
export const PLAN_IT = "Plan it";

/** A follow-up on a goal left a few days, with Plan it unless it's the last line. */
export function FollowUpRow({ followUp, onAsk }: { followUp: FollowUp; onAsk?: (text: string) => void }) {
  return (
    <div className="goal-row goal-follow-up">
      <p>{followUp.line}</p>
      {!followUp.stop && onAsk && (
        <button type="button" className="quiet" onClick={() => onAsk(followUp.prompt)}>
          {PLAN_IT}
        </button>
      )}
    </div>
  );
}

export const YOUR_WEEK = "Your week";

/** Monday's recap on Today: what changed last week and one goal to try. */
export function YourWeekRow({ week, busy, onTrack }: { week: YourWeek; busy: boolean; onTrack: (id: string) => void }) {
  return (
    <div className="goal-row goal-week" aria-label={YOUR_WEEK}>
      <strong>{YOUR_WEEK}</strong>
      <ul>
        {week.lines.map((line) => (
          <li key={line}>{line}</li>
        ))}
      </ul>
      {week.suggestion && (
        <div className="card-buttons">
          <span>Try next: {week.suggestion.title}</span>
          <button type="button" className="quiet" disabled={busy} onClick={() => onTrack(week.suggestion!.id)}>
            Track
          </button>
        </div>
      )}
    </div>
  );
}

export function Goals({ onChange, onAsk }: { onChange?: () => void; onAsk?: (text: string) => void }) {
  const [board, setBoard] = useState<GoalBoard | null>(null);
  const [draft, setDraft] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getGoals().then(setBoard, (err) => setNote(String(err)));
  }, []);

  async function press(acts: GoalAct[]) {
    setBusy(true);
    try {
      const saved = await goalAct(acts);
      setBoard(saved.board);
      setNote(saved.note ?? null);
      if (!saved.note && acts.some((a) => a.act === "add")) setDraft("");
      onChange?.();
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  function add(e: FormEvent) {
    e.preventDefault();
    void press([{ act: "add", text: draft }]);
  }

  if (board === null) {
    return note ? (
      <p className="note" role="status">
        {note}
      </p>
    ) : null;
  }

  return (
    <>
      <div className="card objectives-card" aria-label="Today">
        <span className="xp-level">Today</span>
        {board.week && <YourWeekRow week={board.week} busy={busy} onTrack={(id) => void press([{ act: "track", id }])} />}
        {board.followUp && <FollowUpRow followUp={board.followUp} onAsk={onAsk} />}
        {board.today.length === 0 ? (
          !board.followUp && !board.week && <p className="home-muted">{TODAY_EMPTY}</p>
        ) : (
          board.today.map((g) => (
            <div key={g.id} className="goal-row">
              {g.prompt && <p>{g.prompt}</p>}
              <Summary goal={g} />
            </div>
          ))
        )}
      </div>

      <div className="card objectives-card" aria-label="My goals">
        <span className="xp-level">My goals</span>
        <form className="goal-add" onSubmit={add}>
          <input
            type="text"
            value={draft}
            placeholder={ADD_HINT}
            aria-label="New goal"
            maxLength={200}
            onChange={(e) => setDraft(e.target.value)}
          />
          <button type="submit" disabled={busy}>
            Add
          </button>
        </form>
        {note && (
          <p className="note" role="status">
            {note}
          </p>
        )}
        {board.mine.length === 0 ? (
          <p className="home-muted">{MINE_EMPTY}</p>
        ) : (
          <ul className="goal-list" aria-label="Goals">
            {board.mine.map((g) => (
              <li key={g.id} className="goal-row">
                <Summary goal={g} />
                <div className="card-buttons">
                  {g.plusOne && (
                    <button type="button" className="quiet" disabled={busy} onClick={() => void press([{ act: "bump", id: g.id }])}>
                      +1
                    </button>
                  )}
                  <button type="button" className="quiet" disabled={busy} onClick={() => void press([{ act: "done", id: g.id }])}>
                    Done
                  </button>
                  <button type="button" className="quiet" disabled={busy} onClick={() => void press([{ act: "drop", id: g.id }])}>
                    Drop
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
        {board.suggested.length > 0 && (
          <ul className="goal-list" aria-label="Suggested">
            {board.suggested.map((g) => (
              <li key={g.id} className="goal-row">
                <span>Track goal: {g.title}?</span>
                <div className="card-buttons">
                  <button type="button" disabled={busy} onClick={() => void press([{ act: "track", id: g.id }])}>
                    Track
                  </button>
                  <button type="button" className="quiet" disabled={busy} onClick={() => void press([{ act: "notNow", id: g.id }])}>
                    Not now
                  </button>
                </div>
              </li>
            ))}
          </ul>
        )}
      </div>

      <div className="card objectives-card" aria-label="This season">
        <div className="xp-line">
          <span className="xp-level">This season</span>
          <span className="xp-amount">{board.season.label}</span>
        </div>
        <p className="home-muted">{doneLine(board.season.done)}</p>
        {board.season.badgeLine && <p className="home-muted">{board.season.badgeLine}</p>}
        {board.season.bonus && <p className="goal-bonus">{board.season.bonus}</p>}
        {board.season.badges.length > 0 && (
          <ul className="goal-list" aria-label="Badges">
            {board.season.badges.map((b) => (
              <li key={b} className="goal-row">
                <strong>{b} badge</strong>
              </li>
            ))}
          </ul>
        )}
        {board.season.trophies.length === 0 ? (
          <p className="home-muted">{SHELF_EMPTY}</p>
        ) : (
          <ul className="goal-list" aria-label="Trophies">
            {board.season.trophies.map((g) => (
              <li key={g.id} className="goal-row">
                <strong>{g.title}</strong>
                {g.doneOn && <span className="home-muted"> · {g.doneOn}</span>}
              </li>
            ))}
          </ul>
        )}
      </div>
    </>
  );
}
