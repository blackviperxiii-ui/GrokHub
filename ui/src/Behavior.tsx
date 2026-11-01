import { useEffect, useState, type FormEvent } from "react";
import { getBehavior, saveBehavior, type Behavior as Shape, type BehaviorPage } from "./api";
import { PRODUCT_NAME } from "./brand";
import { Toggle } from "./Toggle";

export const AUTOCOMPLETE_NOTE =
  "Muted ghost text finishes your line in the composer; Tab accepts. Saved for this app's composer, which doesn't show it yet.";
export const QUIET_NOTE = "Leave both empty for no quiet hours.";
export const SCHEDULE_NOTE = `Quiet hours and the budget are saved for scheduled work in this app. Automations here run only when you choose Run now, so nothing waits on them yet. The ${PRODUCT_NAME} cabin keeps its own quiet hours and budget.`;
export const PAUSE_NOTE = "Scheduled work waits until tomorrow once today is over budget. Chat still sends.";

function quietLabel(b: Shape) {
  return b.quietStart ? `${b.quietStart}–${b.quietEnd}` : "None";
}

/** Settings → Behavior: the tray, the composer, and limits on scheduled
 * work, saved in shell.json. `readOnly` is the web app's view. */
export function Behavior({
  tray,
  readOnly = false,
  onCloseToTray,
}: {
  /** This platform has a tray. */
  tray: boolean;
  readOnly?: boolean;
  /** Settings keeps its own copy of the tray choice for General's save. */
  onCloseToTray?: (on: boolean) => void;
}) {
  const [page, setPage] = useState<BehaviorPage | null>(null);
  const [start, setStart] = useState("");
  const [end, setEnd] = useState("");
  const [note, setNote] = useState<string | null>(null);

  function show(next: BehaviorPage) {
    setPage(next);
    setStart(next.behavior.quietStart);
    setEnd(next.behavior.quietEnd);
  }

  useEffect(() => {
    getBehavior().then(show, (err) => setNote(String(err)));
  }, []);

  if (!page) return <p className="note">{note ?? "Loading…"}</p>;
  const loaded = page;
  const b = loaded.behavior;
  const budgetLabel = loaded.budgets.find((c) => c.value === b.dailyTokenBudget)?.label ?? "Off";

  if (readOnly) {
    return (
      <dl aria-label="Behavior">
        {tray && (
          <>
            <dt>Close to tray</dt>
            <dd>{loaded.closeToTray ? "On" : "Off"}</dd>
          </>
        )}
        <dt>Sentence autocomplete</dt>
        <dd>{b.autocomplete ? "On" : "Off"}</dd>
        <dt>Quiet hours</dt>
        <dd>{quietLabel(b)}</dd>
        <dt>Daily token budget</dt>
        <dd>{budgetLabel}</dd>
        {b.dailyTokenBudget > 0 && (
          <>
            <dt>Pause scheduled work over budget</dt>
            <dd>{b.pauseOverBudget ? "On" : "Off"}</dd>
          </>
        )}
      </dl>
    );
  }

  async function save(closeToTray: boolean, next: Shape, done: string) {
    try {
      const saved = await saveBehavior(closeToTray, next);
      show(saved);
      if (saved.closeToTray !== loaded.closeToTray) onCloseToTray?.(saved.closeToTray);
      setNote(done);
    } catch (err) {
      setNote(String(err));
    }
  }

  function change(over: Partial<Shape>) {
    void save(loaded.closeToTray, { ...b, ...over }, "Saved.");
  }

  function saveQuiet(event: FormEvent) {
    event.preventDefault();
    void save(
      loaded.closeToTray,
      { ...b, quietStart: start, quietEnd: end },
      start || end ? "Quiet hours saved." : "No quiet hours.",
    );
  }

  return (
    <div className="behavior" role="group" aria-label="Behavior">
      {tray && (
        <Toggle checked={loaded.closeToTray} onChange={(on) => void save(on, b, "Saved.")}>
          Closing the window keeps {PRODUCT_NAME} in the tray
        </Toggle>
      )}
      <Toggle checked={b.autocomplete} onChange={(on) => change({ autocomplete: on })}>
        Sentence autocomplete
      </Toggle>
      <p className="note">{AUTOCOMPLETE_NOTE}</p>
      <h2>Automations &amp; limits</h2>
      <form onSubmit={saveQuiet}>
        <label>
          Quiet hours start
          <input type="time" value={start} onChange={(e) => setStart(e.target.value)} />
        </label>
        <label>
          Quiet hours end
          <input type="time" value={end} onChange={(e) => setEnd(e.target.value)} />
        </label>
        <p className="note">{QUIET_NOTE}</p>
        <button type="submit">Save quiet hours</button>
      </form>
      <label>
        Daily token budget
        <select
          value={b.dailyTokenBudget}
          onChange={(e) => change({ dailyTokenBudget: Number(e.target.value) })}
        >
          {loaded.budgets.map((c) => (
            <option key={c.value} value={c.value}>
              {c.label}
            </option>
          ))}
        </select>
      </label>
      {b.dailyTokenBudget > 0 && (
        <>
          <Toggle checked={b.pauseOverBudget} onChange={(on) => change({ pauseOverBudget: on })}>
            Pause scheduled work over budget
          </Toggle>
          <p className="note">{PAUSE_NOTE}</p>
        </>
      )}
      <p className="note">{SCHEDULE_NOTE}</p>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
    </div>
  );
}
