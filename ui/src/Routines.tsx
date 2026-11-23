import { useEffect, useState, type MouseEvent } from "react";
import {
  allowRoutine,
  applyRoutineFix,
  dismissRoutineFix,
  listRoutines,
  revokeRoutine,
  runRoutineNow,
  setRoutineOn,
  undoRoutine,
  type RoutineRow,
} from "./routinesApi";
import { Toggle } from "./Toggle";
import { ConfirmRow } from "./ui";
import { ago } from "./Workboard";

export const ROUTINES_NOTE =
  "Routines are fixed steps saved from a chat turn. They run with no model unless a step needs one, so they're fast and cheap.";
export const ROUTINES_EMPTY = "No routines yet. After a chat turn, ask to save what it just did as a routine.";

/** Allow and Undo open on a pointer click only: Enter or Space on a focused button does nothing. */
function pointer(e: MouseEvent) {
  return e.detail > 0;
}

type Asking = { name: string; act: "allow" | "undo" | "fix" };

export function consentLine(row: RoutineRow): string | null {
  const changes = row.changes.join(", ");
  switch (row.consent) {
    case "read_only":
      return "Only reads, so it runs on its own.";
    case "allowed":
      return `Allowed to change things: ${changes}.`;
    case "edited":
      return `Edited since you allowed it. Its changes (${changes}) won't run until you allow them again.`;
    case "not_allowed":
      return `Changes things (${changes}). Those steps won't run until you allow them.`;
    default:
      return null;
  }
}

export function runsLine(row: RoutineRow, now: number): string {
  if (row.runs === 0 || row.lastAtMs === null) return "Not run yet.";
  const runs = row.runs === 1 ? "1 run" : `${row.runs} runs`;
  const calls = row.modelCalls === 1 ? "1 model call" : `${row.modelCalls} model calls`;
  const last = row.lastOk ? "finished" : "stopped";
  return `${runs}, ${calls}. Last ${ago(row.lastAtMs, now)}, ${last}.`;
}

/** Automations → Routines: each saved routine with what starts it, whether
 * its change steps are allowed, on or off, and its runs and model calls. */
export function Routines() {
  const [rows, setRows] = useState<RoutineRow[] | null>(null);
  const [hidden, setHidden] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [asking, setAsking] = useState<Asking | null>(null);
  const [results, setResults] = useState<Record<string, string>>({});

  useEffect(() => {
    // Not served to a paired browser: the section stays away there.
    listRoutines().then(setRows, () => setHidden(true));
  }, []);

  async function act(name: string, work: () => Promise<RoutineRow[]>) {
    setBusy(name);
    setAsking(null);
    try {
      setRows(await work());
      setNote(null);
    } catch (err) {
      setNote(String(err));
      // A routine saved since the list loaded is refused: load it again, so
      // the next Allow shows what it does now.
      await listRoutines().then(setRows, () => {});
    } finally {
      setBusy(null);
    }
  }

  /** Undo or Apply fix: a status line, then the list again. */
  async function change(name: string, work: () => Promise<string>) {
    setBusy(name);
    setAsking(null);
    try {
      setNote(await work());
      setRows(await listRoutines());
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(null);
    }
  }

  async function run(name: string) {
    setBusy(name);
    // The list loads again after the run: an open Allow mustn't change under the person.
    setAsking(null);
    try {
      const got = await runRoutineNow(name);
      setResults((now) => ({ ...now, [name]: got.error || got.text }));
      setRows(await listRoutines());
    } catch (err) {
      setResults((now) => ({ ...now, [name]: String(err) }));
    } finally {
      setBusy(null);
    }
  }

  function ask(e: MouseEvent, next: Asking) {
    if (!pointer(e) || busy !== null) return;
    setAsking(next);
  }

  if (hidden || rows === null) return null;
  const now = Date.now();

  return (
    <section className="routines" aria-label="Routines">
      <h3>Routines</h3>
      <p className="note">{ROUTINES_NOTE}</p>
      {rows.length === 0 && <p className="empty">{ROUTINES_EMPTY}</p>}
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      <ul className="rows">
        {rows.map((row) => {
          const broken = row.problems.length > 0;
          const consent = consentLine(row);
          const isAsking = asking?.name === row.name ? asking.act : null;
          // A Run now's full report, else the newest run's last line.
          const shown = results[row.name] ?? row.lastLine;
          return (
            <li key={row.name} className="card automation routine" aria-label={row.name}>
              <div className="card-title">{row.name}</div>
              {row.about && <div className="card-status">{row.about}</div>}
              {broken ? (
                <div className="card-health" role="alert">
                  Can't run: {row.problems.join(" ")}
                </div>
              ) : (
                <>
                  <div className="card-status">
                    {row.triggers.join(" · ") || "No triggers"} · {row.steps === 1 ? "1 step" : `${row.steps} steps`}
                  </div>
                  {consent && <div className="card-check">{consent}</div>}
                  <div className="card-status">{runsLine(row, now)}</div>
                  {shown && <pre className="card-detail">{shown}</pre>}
                  <Toggle
                    checked={row.on}
                    disabled={busy !== null}
                    onChange={(next) => void act(row.name, () => setRoutineOn(row.name, next))}
                  >
                    {`Run ${row.name} on its triggers`}
                  </Toggle>
                </>
              )}
              {row.fix && (
                <div className="card-check routine-fix" role="group" aria-label={`Suggested fix for ${row.name}`}>
                  <p>
                    {`The same repair to step ${row.fix.step} worked in ${row.fix.seen} runs. Make it part of the routine?`}
                  </p>
                  <pre className="card-detail">{`Now:  ${row.fix.before}\nFixed: ${row.fix.after}`}</pre>
                </div>
              )}
              {isAsking === "fix" && row.fix && (
                <ConfirmRow
                  question={`Fix step ${row.fix.step} in ${row.name}?`}
                  detail="Undo takes it back."
                  action="Fix"
                  busy={busy !== null}
                  onConfirm={() => void change(row.name, () => applyRoutineFix(row.name))}
                  onCancel={() => setAsking(null)}
                />
              )}
              {isAsking === "allow" && row.hash && (
                <>
                  <pre className="card-detail">{row.changeDetails.join("\n")}</pre>
                  <ConfirmRow
                    question={`Allow ${row.name} to change things?`}
                    detail="The steps above will run on their own, as shown. Editing the routine takes this back."
                    action="Allow"
                    busy={busy !== null}
                    onConfirm={() => void act(row.name, () => allowRoutine(row.name, row.hash ?? ""))}
                    onCancel={() => setAsking(null)}
                  />
                </>
              )}
              {isAsking === "undo" && (
                <ConfirmRow
                  question={`Undo the newest change to ${row.name}?`}
                  detail="A routine that was just saved is removed. Its text stays in the change history."
                  action="Undo"
                  busy={busy !== null}
                  onConfirm={() => void change(row.name, () => undoRoutine(row.name))}
                  onCancel={() => setAsking(null)}
                />
              )}
              {isAsking === null && (
                <div className="card-buttons">
                  {row.fix && (
                    <>
                      <button
                        type="button"
                        disabled={busy !== null}
                        onClick={(e) => ask(e, { name: row.name, act: "fix" })}
                      >
                        Apply fix…
                      </button>
                      <button
                        type="button"
                        className="quiet"
                        disabled={busy !== null}
                        onClick={() => void act(row.name, () => dismissRoutineFix(row.name))}
                      >
                        Dismiss fix
                      </button>
                    </>
                  )}
                  {!broken && (
                    <button type="button" disabled={busy !== null} onClick={() => void run(row.name)}>
                      {busy === row.name ? "Running…" : "Run now"}
                    </button>
                  )}
                  {(row.consent === "not_allowed" || row.consent === "edited") && (
                    <button
                      type="button"
                      className="quiet"
                      disabled={busy !== null}
                      onClick={(e) => ask(e, { name: row.name, act: "allow" })}
                    >
                      Allow changes…
                    </button>
                  )}
                  {row.consent === "allowed" && (
                    <button
                      type="button"
                      className="quiet"
                      disabled={busy !== null}
                      onClick={() => void act(row.name, () => revokeRoutine(row.name))}
                    >
                      Take back
                    </button>
                  )}
                  <button
                    type="button"
                    className="quiet"
                    disabled={busy !== null}
                    onClick={(e) => ask(e, { name: row.name, act: "undo" })}
                  >
                    Undo…
                  </button>
                </div>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
