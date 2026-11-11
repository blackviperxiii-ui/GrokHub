import { useCallback, useEffect, useRef, useState } from "react";
import { codeDeployPlan, codeDeployRun, codeRunStop, onCodeDeployRun, type CodeDeployPlan } from "./api";
import { useCardFocus } from "./ui";

export const DEPLOY_TITLE = "Deploy";
export const DEPLOY_CONFIRM = "Run this deploy command?";
export const DEPLOY_OFF = "Deploy is off for this project. Turn it on in Settings → Code.";
export const DEPLOY_STOPPED = "Deploy stopped.";
export const DEPLOY_RULES = "Your project's secrets are set as environment variables for the run and hidden in the output and the activity list.";

/** The Code tab's deploy section: one button, and a confirm card that shows the exact command. */
export function CodeDeploy({ onDeployed }: { onDeployed?: () => void }) {
  const [plan, setPlan] = useState<CodeDeployPlan | null>(null);
  const [confirming, setConfirming] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [log, setLog] = useState("");
  const [stopping, setStopping] = useState(false);
  // True from a press on Stop until the next run starts.
  const stopAsked = useRef(false);
  const cancel = useCardFocus<HTMLButtonElement>(confirming);

  const show = useCallback((next: CodeDeployPlan) => {
    setPlan(next);
    // A card for a command that would no longer run goes away, so nobody confirms the old one.
    setConfirming((open) => (open && next.deployEnabled && !next.blocked && next.command === open ? open : null));
  }, []);

  const refresh = useCallback(async () => {
    const next = await codeDeployPlan();
    show(next);
    return next;
  }, [show]);

  // Runs each time the tab shows, so a card left open for a command changed in Settings goes too.
  useEffect(() => {
    codeDeployPlan().then(show, (err) => setNote(String(err)));
  }, [show]);

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
    stopAsked.current = false;
    try {
      const done = await codeDeployRun(command, confirmed);
      setLog(done.output + (done.cut ? "\n… cut here; the output is too long to show in full." : ""));
      const stopped = stopAsked.current && done.exit !== 0;
      setNote(done.exit === 0 ? "Deploy finished." : stopped ? DEPLOY_STOPPED : `Deploy exited with ${done.exit}.`);
    } catch (err) {
      setNote(stopAsked.current ? DEPLOY_STOPPED : String(err));
    } finally {
      setBusy(false);
      onDeployed?.();
      refresh().catch(() => {});
    }
  }

  /** Stop the running deploy and everything it started; the run then ends by itself. */
  async function stopRun() {
    setStopping(true);
    stopAsked.current = true;
    try {
      await codeRunStop();
    } catch (err) {
      stopAsked.current = false;
      setNote(String(err));
    } finally {
      setStopping(false);
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
        {busy && (
          <button type="button" className="quiet" onClick={() => void stopRun()} disabled={stopping}>
            {stopping ? "Stopping…" : "Stop"}
          </button>
        )}
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
      {(log || busy) && (
        <pre className="code-log" tabIndex={0} role="log" aria-label="Deploy output">
          {log || "Running…"}
        </pre>
      )}
    </div>
  );
}
