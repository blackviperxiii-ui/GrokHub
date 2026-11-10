import { useCallback, useEffect, useState } from "react";
import {
  codeActivity,
  codeNewBranch,
  codeShipPlan,
  codeShipRun,
  type CodeActivityEntry,
  type CodeShipAction,
  type CodeShipPlan,
} from "./api";

export const SHIP_TITLE = "Push and PR";
export const SHIP_CONFIRM_PUSH = "Push this branch?";
export const SHIP_CONFIRM_PR = "Open this PR?";
export const SHIP_RULES = "Never pushes main or the default branch, never force-pushes, never deletes a branch.";
export const SHIP_ACTIVITY = "Activity";
export const SHIP_NO_ACTIVITY = "No pushes or PRs from this tab yet.";

const KIND_LABEL: Record<CodeActivityEntry["kind"], string> = {
  push: "Push",
  pr: "PR",
  branch: "Branch",
  blocked: "Blocked",
  deploy: "Deploy",
  secret: "Secret",
};

/** One activity line's time, in the person's own clock. */
export function activityTime(ms: number): string {
  return new Date(ms).toLocaleString(undefined, { month: "short", day: "numeric", hour: "numeric", minute: "2-digit" });
}

/** The Code tab's push, PR and activity section. Every push goes through the guard in the app. */
export function CodeShip({ onShipped }: { onShipped?: () => void }) {
  const [push, setPush] = useState<CodeShipPlan | null>(null);
  const [pr, setPr] = useState<CodeShipPlan | null>(null);
  const [confirming, setConfirming] = useState<CodeShipPlan | null>(null);
  const [branchName, setBranchName] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [output, setOutput] = useState("");
  const [activity, setActivity] = useState<CodeActivityEntry[]>([]);

  const refresh = useCallback(async () => {
    const [nextPush, nextPr, log] = await Promise.all([codeShipPlan("push"), codeShipPlan("pr"), codeActivity()]);
    setPush(nextPush);
    setPr(nextPr);
    setActivity(log);
    return { push: nextPush, pr: nextPr };
  }, []);

  useEffect(() => {
    Promise.all([codeShipPlan("push"), codeShipPlan("pr"), codeActivity()]).then(
      ([nextPush, nextPr, log]) => {
        setPush(nextPush);
        setPr(nextPr);
        setActivity(log);
      },
      (err) => setNote(String(err)),
    );
  }, []);

  async function start(action: CodeShipAction) {
    if (busy) return;
    setNote(null);
    setOutput("");
    try {
      const plans = await refresh();
      const plan = action === "push" ? plans.push : plans.pr;
      if (plan.blocked) {
        setNote(plan.blocked);
        return;
      }
      if (plan.needsConfirm) {
        setConfirming(plan);
        return;
      }
      await run(action, false);
    } catch (err) {
      setNote(String(err));
    }
  }

  async function run(action: CodeShipAction, confirmed: boolean) {
    setConfirming(null);
    setBusy(true);
    try {
      const done = await codeShipRun(action, confirmed);
      setNote(done.summary);
      setOutput(done.output);
      onShipped?.();
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
      refresh().catch(() => {});
    }
  }

  async function makeBranch() {
    const name = branchName.trim();
    if (!name || busy) return;
    setBusy(true);
    setNote(null);
    try {
      const made = await codeNewBranch(name);
      setBranchName("");
      setNote(`On ${made} now.`);
      onShipped?.();
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
      refresh().catch(() => {});
    }
  }

  const base = pr?.autoBase || push?.autoBase || "beta";
  const modeNote =
    push?.mode === "autoBeta"
      ? `Auto-beta: a branch made here pushes into ${base} without asking. Everything else asks.`
      : "Ask: every push and PR shows the exact command first.";
  return (
    <div className="code-ship" role="group" aria-label={SHIP_TITLE}>
      <h3>{SHIP_TITLE}</h3>
      <p className="note">
        {modeNote} {SHIP_RULES}
      </p>
      <div className="code-test-row">
        <label>
          New branch
          <input
            value={branchName}
            spellCheck={false}
            autoComplete="off"
            placeholder="code/fix-typo"
            disabled={busy}
            onChange={(e) => setBranchName(e.target.value)}
          />
        </label>
        <button type="button" onClick={() => void makeBranch()} disabled={busy || !branchName.trim()}>
          Make branch
        </button>
      </div>
      <div className="card-buttons code-ship-buttons">
        <button type="button" onClick={() => void start("push")} disabled={busy || !push}>
          {busy ? "Working…" : push?.branch ? `Push ${push.branch}` : "Push"}
        </button>
        {pr?.prUrl ? (
          <span className="note">PR open: {pr.prUrl}</span>
        ) : (
          <button type="button" onClick={() => void start("pr")} disabled={busy || !pr}>
            Open PR into {base}
          </button>
        )}
      </div>
      {confirming && (
        <div
          className="card confirm-window"
          role="group"
          aria-label={confirming.action === "push" ? SHIP_CONFIRM_PUSH : SHIP_CONFIRM_PR}
        >
          <p>{confirming.action === "push" ? SHIP_CONFIRM_PUSH : SHIP_CONFIRM_PR}</p>
          <pre className="code-command" aria-label="Command to run">
            {confirming.command}
          </pre>
          <div className="card-buttons">
            <button type="button" onClick={() => void run(confirming.action, true)} disabled={busy}>
              {confirming.action === "push" ? "Push" : "Open PR"}
            </button>
            <button type="button" className="quiet" onClick={() => setConfirming(null)} disabled={busy}>
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
        <pre className="code-log" aria-label="Push output">
          {output}
        </pre>
      )}
      <h4>{SHIP_ACTIVITY}</h4>
      {activity.length === 0 ? (
        <p className="empty">{SHIP_NO_ACTIVITY}</p>
      ) : (
        <ul className="rows code-activity" aria-label={SHIP_ACTIVITY}>
          {activity.map((e) => (
            <li key={`${e.tsMs}-${e.kind}-${e.branch}`} className={`code-activity-row ${e.kind}`}>
              <span className="code-activity-kind">{KIND_LABEL[e.kind]}</span>
              <span className="code-activity-branch">{e.branch}</span>
              <span className="note code-activity-summary">{e.summary}</span>
              <time className="note" dateTime={new Date(e.tsMs).toISOString()}>
                {activityTime(e.tsMs)}
              </time>
              <code className="code-activity-command">{e.command}</code>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
