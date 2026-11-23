import { useEffect, useState, type KeyboardEvent } from "react";
import {
  commandBoxApprove,
  commandBoxCheck,
  commandBoxForget,
  commandBoxView,
  type CommandBoxView,
} from "./api";
import { COMMAND_DESTRUCTIVE, COMMAND_FIRST } from "./commandText";

export const COMMAND_TITLE = "Command line";
export const COMMAND_ABOUT =
  "One shell line in the project folder. It goes to Chat, where the agent runs it through the same approvals as anything else it does.";
export { COMMAND_DESTRUCTIVE, COMMAND_FIRST } from "./commandText";
export const COMMAND_FIRST_NOTE =
  "After a yes it's remembered for this project. Changed text asks again. A line with a key or token in it is never remembered; it asks every time.";
export const COMMAND_SENT = "Sent to Chat.";
export const COMMAND_NONE_YET = "Nothing run yet. Type a command above; it runs in the project folder.";
/** The most past lines the page keeps for the up arrow and the list. */
export const COMMAND_HISTORY_KEEP = 50;
/** How many past lines the list shows. */
const HISTORY_SHOWN = 6;

/** The cabin's Command page in the desktop app: a `$` box bound to the
 * project folder, the lines run so far, and the lines remembered for this
 * project. Run hands the agent's message to `onRun`, which sends it in Chat,
 * as `/sh` in Chat does. */
export function CommandBox({ onRun }: { onRun: (prompt: string) => void }) {
  const [view, setView] = useState<CommandBoxView | null>(null);
  const [line, setLine] = useState("");
  const [history, setHistory] = useState<string[]>([]);
  /** Where the up arrow is in `history`; `null` while typing a new line. */
  const [back, setBack] = useState<number | null>(null);
  const [asking, setAsking] = useState<{ command: string; destructive: boolean } | null>(null);
  const [note, setNote] = useState<{ text: string; error: boolean } | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    commandBoxView().then(setView, (err) => setNote({ text: String(err), error: true }));
  }, []);

  function sent(command: string, prompt: string) {
    setHistory((now) => [...now.filter((c) => c !== command), command].slice(-COMMAND_HISTORY_KEEP));
    setLine("");
    setBack(null);
    setAsking(null);
    setNote({ text: COMMAND_SENT, error: false });
    onRun(prompt);
  }

  async function run() {
    const typed = line.trim();
    if (!typed || busy) return;
    setBusy(true);
    setNote(null);
    setAsking(null);
    try {
      const verdict = await commandBoxCheck(typed);
      if (verdict.verdict === "queue") sent(verdict.command, verdict.prompt);
      else if (verdict.verdict === "confirm")
        setAsking({ command: verdict.command, destructive: verdict.destructive });
      else setNote({ text: verdict.message, error: true });
    } catch (err) {
      setNote({ text: String(err), error: true });
    } finally {
      setBusy(false);
    }
  }

  async function approve() {
    if (!asking || busy) return;
    const { command } = asking;
    setBusy(true);
    try {
      const prompt = await commandBoxApprove(command);
      sent(command, prompt);
      commandBoxView().then(setView, () => {});
    } catch (err) {
      setAsking(null);
      setNote({ text: String(err), error: true });
    } finally {
      setBusy(false);
    }
  }

  async function forget() {
    setBusy(true);
    try {
      setView(await commandBoxForget());
      setNote({ text: "Forgot them. Each command asks again on its next run.", error: false });
    } catch (err) {
      setNote({ text: String(err), error: true });
    } finally {
      setBusy(false);
    }
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Enter") {
      event.preventDefault();
      void run();
      return;
    }
    if (history.length === 0 || (event.key !== "ArrowUp" && event.key !== "ArrowDown")) return;
    event.preventDefault();
    const last = history.length - 1;
    const next =
      event.key === "ArrowUp" ? (back === null ? last : Math.max(0, back - 1)) : back === null || back >= last ? null : back + 1;
    setBack(next);
    setLine(next === null ? "" : history[next]);
    setAsking(null);
  }

  const shown = [...history].reverse().slice(0, HISTORY_SHOWN);
  return (
    <section className="code command-box" aria-label={COMMAND_TITLE}>
      <header className="code-head">
        <div>
          <div className="card-title">{COMMAND_TITLE}</div>
          <div className="note">{view ? `Runs in ${view.project}` : "Reading the project folder…"}</div>
        </div>
      </header>
      <p className="note">{COMMAND_ABOUT}</p>
      <div className="code-test-row">
        <label className="command-box-line">
          <span className="command-box-prompt" aria-hidden="true">
            $
          </span>
          <input
            aria-label="Command"
            value={line}
            spellCheck={false}
            autoComplete="off"
            placeholder="git status"
            disabled={busy}
            onChange={(e) => {
              setLine(e.target.value);
              setBack(null);
              setAsking(null);
            }}
            onKeyDown={onKeyDown}
          />
        </label>
        <button type="button" onClick={() => void run()} disabled={busy || !line.trim()}>
          Run
        </button>
      </div>
      {note && (
        <p className={note.error ? "note warn" : "note"} role={note.error ? "alert" : "status"}>
          {note.text}
        </p>
      )}
      {asking && (
        <div className="card confirm-window" role="group" aria-label={COMMAND_FIRST}>
          <p>{asking.destructive ? COMMAND_DESTRUCTIVE : COMMAND_FIRST}</p>
          <pre className="code-command" aria-label="Command to run">
            {asking.command}
          </pre>
          {!asking.destructive && <p className="confirm-keeps">{COMMAND_FIRST_NOTE}</p>}
          <div className="card-buttons">
            <button type="button" onClick={() => void approve()} disabled={busy}>
              Run it
            </button>
            <button type="button" className="quiet" onClick={() => setAsking(null)} disabled={busy}>
              Cancel
            </button>
          </div>
        </div>
      )}
      <div className="code-tests">
        <h3>History</h3>
        {shown.length === 0 ? (
          <p className="empty">{COMMAND_NONE_YET}</p>
        ) : (
          <ul className="rows command-box-list" aria-label="History">
            {shown.map((cmd) => (
              <li key={cmd}>
                <button
                  type="button"
                  className="code-file quiet"
                  title="Put it back in the box"
                  onClick={() => {
                    setLine(cmd);
                    setAsking(null);
                  }}
                >
                  {cmd}
                </button>
              </li>
            ))}
          </ul>
        )}
      </div>
      {view && view.remembered.length > 0 && (
        <div className="code-tests">
          <h3>Remembered for this project</h3>
          <ul className="rows command-box-list" aria-label="Remembered for this project">
            {view.remembered.map((cmd) => (
              <li key={cmd}>
                <code>{cmd}</code>
              </li>
            ))}
          </ul>
          <div className="card-buttons">
            <button type="button" className="quiet" onClick={() => void forget()} disabled={busy}>
              Forget remembered commands
            </button>
          </div>
        </div>
      )}
    </section>
  );
}
