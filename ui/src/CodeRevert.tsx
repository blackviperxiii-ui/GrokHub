import { useCallback, useEffect, useState } from "react";
import { codeRevertList, codeRevertPlan, codeRevertRun, type CodeCommit, type CodeRevertPlan } from "./codeRevertApi";
import { useCardFocus } from "./ui";

export const REVERT_TITLE = "Roll back";
export const REVERT_RULES =
  "Undoes a commit with a new revert commit, then pushes it through the push guard. Always asks first; never resets or force-pushes.";
export const REVERT_CONFIRM = "Revert this commit?";
export const REVERT_MERGE = "Merge: not reverted here";
export const REVERT_EMPTY = "No commits on this branch yet.";

/** The Code tab's rollback: the branch's newest commits, each with Revert, confirmed every time. */
export function CodeRevert({ onReverted, refreshKey = 0 }: { onReverted?: () => void; refreshKey?: number }) {
  const [commits, setCommits] = useState<CodeCommit[] | null>(null);
  const [confirming, setConfirming] = useState<CodeRevertPlan | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [output, setOutput] = useState("");
  const cancel = useCardFocus<HTMLButtonElement>(confirming);

  const refresh = useCallback(async () => {
    setCommits(await codeRevertList());
  }, []);

  useEffect(() => {
    codeRevertList().then(setCommits, (err) => setNote(String(err)));
  }, [refreshKey]);

  async function start(c: CodeCommit) {
    if (busy) return;
    setNote(null);
    setOutput("");
    try {
      const plan = await codeRevertPlan(c.sha);
      if (plan.blocked) {
        setConfirming(null);
        setNote(plan.blocked);
        return;
      }
      setConfirming(plan);
    } catch (err) {
      setNote(String(err));
    }
  }

  async function run(plan: CodeRevertPlan) {
    setConfirming(null);
    setBusy(true);
    try {
      const done = await codeRevertRun(plan);
      setNote(done.summary);
      setOutput(done.output);
      onReverted?.();
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
      refresh().catch(() => {});
    }
  }

  return (
    <div className="code-revert" role="group" aria-label={REVERT_TITLE}>
      <h3>{REVERT_TITLE}</h3>
      <p className="note">{REVERT_RULES}</p>
      {commits && commits.length === 0 && <p className="empty">{REVERT_EMPTY}</p>}
      {commits && commits.length > 0 && (
        <ul className="rows" style={{ paddingLeft: 0, listStyle: "none" }} aria-label="Commits on this branch">
          {commits.map((c) => (
            <li key={c.sha} style={{ display: "flex", alignItems: "center", gap: "0.75rem", padding: "0.25rem 0" }}>
              <code>{c.short}</code>
              <span style={{ flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                {c.subject}
              </span>
              {c.merge ? (
                <span className="note">{REVERT_MERGE}</span>
              ) : (
                <button
                  type="button"
                  className="quiet"
                  aria-label={`Revert ${c.short}`}
                  onClick={() => void start(c)}
                  disabled={busy}
                >
                  Revert
                </button>
              )}
            </li>
          ))}
        </ul>
      )}
      {confirming && (
        <div className="card confirm-window" role="group" aria-label={REVERT_CONFIRM}>
          <p>
            {REVERT_CONFIRM} {confirming.short} {confirming.subject}
          </p>
          <pre className="code-command" aria-label="Commands to run">
            {`${confirming.revertCommand}\n${confirming.pushCommand}`}
          </pre>
          <div className="card-buttons">
            <button type="button" className="danger" onClick={() => void run(confirming)} disabled={busy}>
              Revert and push
            </button>
            <button ref={cancel} type="button" className="quiet" onClick={() => setConfirming(null)} disabled={busy}>
              Cancel
            </button>
          </div>
        </div>
      )}
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      {output && (
        <pre className="code-log" tabIndex={0} role="log" aria-label="Revert output">
          {output}
        </pre>
      )}
    </div>
  );
}
