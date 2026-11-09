import { useEffect, useState } from "react";
import { getLearned, learnedAct, type LearnedAct, type LearnedRow, type LearnedStage, type LearnedView } from "./api";
import { PRODUCT_NAME } from "./brand";

export const LEARNED_NOTE = `What ${PRODUCT_NAME} worked out from how you use it: which quick chips and cards you take, and how runs go. It stays on this computer and holds counts only, never what you typed. Forget clears a line so it starts fresh. That's wrong stops a line from being counted or used again.`;
export const LEARNED_EMPTY = "Nothing learned yet. It builds up as you use quick chips, cards and runs.";

const STAGE: Record<LearnedStage, string> = {
  watching: "Still watching",
  suggesting: "Sure enough to suggest",
  acting: "Sure enough to act",
  measuring: "A measurement",
  wrong: "Marked wrong",
};

function proof(row: LearnedRow) {
  if (row.stage === "wrong") return STAGE.wrong;
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
