import { useCallback, useEffect, useState } from "react";
import { codeDeployPlan, codeDeployRun, onCodeDeployRun, type CodeDeployPlan } from "./api";

export const DEPLOY_TITLE = "Deploy";
export const DEPLOY_CONFIRM = "Run this deploy command?";
export const DEPLOY_OFF = "Deploy is off for this project. Turn it on in Settings → Code.";
export const DEPLOY_RULES = "Your project's secrets are set as environment variables for the run and hidden in the output and the activity list.";

/** The Code tab's deploy section: one button, and a confirm card that shows the exact command. */
export function CodeDeploy({ onDeployed }: { onDeployed?: () => void }) {
  const [plan, setPlan] = useState<CodeDeployPlan | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [log, setLog] = useState("");

  const refresh = useCallback(async () => {
    const next = await codeDeployPlan();
    setPlan(next);
    return next;
  }, []);

  useEffect(() => {
    codeDeployPlan().then(setPlan, (err) => setNote(String(err)));
  }, []);

  useEffect(() => {
    let gone = false;
    let unlisten = () => {};
    void onCodeDeployRun((chunk) => {
      if (gone || !chunk.text) return;
      setLog((log) => log + chunk.text);
    }).then(
      (off) => {
        if (gone) off();
        else unlisten = off;
      },
      () => {},
    );
    return () => {
      gone = true;
      unlisten();
    };
  }, []);

  async function start() {
    if (busy) return;
    setNote(null);
    try {
      const next = await refresh();
      if (next.blocked) {
        setNote(next.blocked);
        return;
      }
      if (next.needsConfirm) {
        setConfirming(next.command);
        return;
      }
      await run(next.command, false);
    } catch (err) {
      setNote(String(err));
    }
  }

  async function run(command: string, confirmed: boolean) {
    setConfirming(null);
    setBusy(true);
    setLog("");
    setNote(null);
    try {
      const done = await codeDeployRun(command, confirmed);
      setLog(done.output + (done.cut ? "\n… cut here; the output is too long to show in full." : ""));
      setNote(done.exit === 0 ? "Deploy finished." : `Deploy exited with ${done.exit}.`);
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
      onDeployed?.();
      refresh().catch(() => {});
    }
  }

  const off = plan !== null && !plan.deployEnabled;
  const modeNote =
    plan?.mode === "autoBeta"
      ? "Auto-beta: a remembered deploy runs without asking, and asks again if its command changes."
      : "Ask: every deploy shows the exact command first.";
  return (
    <div className="code-deploy" role="group" aria-label={DEPLOY_TITLE}>
      <h3>{DEPLOY_TITLE}</h3>
      <p className="note">{off ? DEPLOY_OFF : `${modeNote} ${DEPLOY_RULES}`}</p>
      <div className="card-buttons">
        <button type="button" onClick={() => void start()} disabled={busy || !plan || off}>
          {busy ? "Deploying…" : "Deploy"}
        </button>
      </div>
      {confirming && (
        <div className="card confirm-window" role="group" aria-label={DEPLOY_CONFIRM}>
          <p>{DEPLOY_CONFIRM}</p>
          <pre className="code-command" aria-label="Command to run">
            {confirming}
          </pre>
          <div className="card-buttons">
            <button type="button" onClick={() => void run(confirming, true)} disabled={busy}>
              Deploy
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
      {(log || busy) && (
        <pre className="code-log" aria-label="Deploy output">
          {log || "Running…"}
        </pre>
      )}
    </div>
  );
}
