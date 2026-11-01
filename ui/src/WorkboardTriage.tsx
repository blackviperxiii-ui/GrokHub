import { useState, type FormEvent } from "react";
import { getPlaybooks, RECUR_CHOICES, type BoardAct, type BoardCard, type BoardLane, type PlaybookRow } from "./api";

/** The priority chip's choices, as the shell stores them. */
export const PRIORITY_CHOICES = [
  ["high", "High"],
  ["normal", "Normal"],
  ["low", "Low"],
] as const;

export type Priority = (typeof PRIORITY_CHOICES)[number][0];

/** A stored priority ("" is normal) as the chip's value. */
export function priorityValue(p: string): Priority {
  return p === "high" || p === "low" ? p : "normal";
}

/** The compact card's chip for a priority other than normal. */
export function priorityChip(p: string): string | null {
  const v = priorityValue(p);
  return v === "normal" ? null : (PRIORITY_CHOICES.find(([k]) => k === v)?.[1] ?? null);
}

export const QUEUED_NOTE = "Queued. The cabin starts it once nothing else is running and no game is up.";
export const STEPS_HELP = "One step a line. [x] done, [>] doing, [-] skipped. The agent works from your version.";

type OnAct = (what: BoardAct, done?: string) => Promise<boolean>;

/** Under an open card: Agent: do this (Now or Queue), the priority chip,
 * Block with a reason or Unblock, your edit of the steps, and on the desktop
 * Start a flow and Make it recurring. */
export function CardTriage({
  card,
  lane,
  busy,
  desktop,
  onAct,
  onWork,
  onPlaybook,
  onRecur,
  onError,
}: {
  card: BoardCard;
  lane: BoardLane;
  busy: boolean;
  /** Playbooks live on the computer; the web app leaves Start a flow out. */
  desktop: boolean;
  onAct: OnAct;
  onWork: () => void;
  onPlaybook: (id: string, name: string) => void;
  onRecur: (choice: number) => void;
  onError: (err: unknown) => void;
}) {
  const [blocking, setBlocking] = useState<string | null>(null);
  const [steps, setSteps] = useState<string | null>(null);
  const [playbooks, setPlaybooks] = useState<PlaybookRow[] | null>(null);
  const [pick, setPick] = useState("");
  const [recur, setRecur] = useState(0);
  const handOver = lane !== "done" && lane !== "blocked";

  async function block(event: FormEvent) {
    event.preventDefault();
    if (blocking === null || !blocking.trim()) return;
    if (await onAct({ kind: "block", reason: blocking }, "Blocked. The reason is saved where the work left off.")) {
      setBlocking(null);
    }
  }

  async function saveSteps(event: FormEvent) {
    event.preventDefault();
    if (steps === null) return;
    if (await onAct({ kind: "steps", text: steps }, "Steps saved. The agent works from your version.")) {
      setSteps(null);
    }
  }

  async function loadPlaybooks() {
    try {
      setPlaybooks((await getPlaybooks()).rows);
    } catch (err) {
      onError(err);
    }
  }

  const chosen = playbooks?.find((p) => p.id === pick);

  return (
    <div className="work-triage">
      {card.flow && <p className="card-status">{card.flow}</p>}
      {handOver && (
        <div className="card-buttons" role="group" aria-label="Agent: do this">
          <span className="card-status">Agent: do this</span>
          <button type="button" disabled={busy} onClick={onWork}>
            Now
          </button>
          {card.queued ? (
            <button
              type="button"
              className="quiet"
              disabled={busy}
              onClick={() => void onAct({ kind: "unqueue" }, "Taken off the queue.")}
            >
              Take off the queue
            </button>
          ) : (
            lane !== "doing" && (
              <button
                type="button"
                className="quiet"
                disabled={busy}
                title="Starts on its own once nothing else is running and no game is up"
                onClick={() => void onAct({ kind: "queue" }, QUEUED_NOTE)}
              >
                Queue
              </button>
            )
          )}
        </div>
      )}
      <div className="card-buttons">
        <label className="card-status">
          Priority{" "}
          <select
            aria-label="Priority"
            disabled={busy}
            value={priorityValue(card.priority)}
            onChange={(e) => void onAct({ kind: "priority", priority: e.target.value as Priority })}
          >
            {PRIORITY_CHOICES.map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </label>
        {lane === "blocked" ? (
          <button
            type="button"
            disabled={busy}
            onClick={() => void onAct({ kind: "unblock" }, "Unblocked. It is back in Todo.")}
          >
            Unblock
          </button>
        ) : (
          blocking === null && (
            <button type="button" className="quiet" disabled={busy} onClick={() => setBlocking("")}>
              Block…
            </button>
          )
        )}
        {steps === null && (
          <button type="button" className="quiet" disabled={busy} onClick={() => setSteps(card.stepsText)}>
            {card.steps.length > 0 ? "Edit steps" : "Add steps"}
          </button>
        )}
      </div>
      {blocking !== null && (
        <form className="board-block" aria-label="Block" onSubmit={(e) => void block(e)}>
          <input
            aria-label="What is it waiting on?"
            placeholder="What is it waiting on?"
            value={blocking}
            onChange={(e) => setBlocking(e.target.value)}
          />
          <div className="card-buttons">
            <button type="submit" disabled={busy || !blocking.trim()}>
              Block
            </button>
            <button type="button" className="quiet" onClick={() => setBlocking(null)}>
              Cancel
            </button>
          </div>
        </form>
      )}
      {steps !== null && (
        <form className="board-steps-edit" aria-label="Edit steps" onSubmit={(e) => void saveSteps(e)}>
          <textarea aria-label="Steps" rows={4} value={steps} onChange={(e) => setSteps(e.target.value)} />
          <p className="card-status">{STEPS_HELP}</p>
          <div className="card-buttons">
            <button type="submit" disabled={busy}>
              Save steps
            </button>
            <button type="button" className="quiet" onClick={() => setSteps(null)}>
              Cancel
            </button>
          </div>
        </form>
      )}
      {handOver && desktop && (
        <div className="card-buttons" role="group" aria-label="Start a flow">
          {playbooks === null ? (
            <button type="button" className="quiet" disabled={busy} onClick={() => void loadPlaybooks()}>
              Start a flow
            </button>
          ) : (
            <>
              <select aria-label="Playbook" value={pick} onChange={(e) => setPick(e.target.value)}>
                <option value="">Pick a playbook</option>
                {playbooks.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </select>
              <button
                type="button"
                disabled={busy || !chosen}
                onClick={() => chosen && onPlaybook(chosen.id, chosen.name)}
              >
                Start
              </button>
            </>
          )}
        </div>
      )}
      {handOver && card.kind !== "Automation" && (
        <div className="card-buttons" role="group" aria-label="Make it recurring">
          <select aria-label="Schedule" value={recur} onChange={(e) => setRecur(Number(e.target.value))}>
            {RECUR_CHOICES.map((label, i) => (
              <option key={label} value={i}>
                {label}
              </option>
            ))}
          </select>
          <button type="button" className="quiet" disabled={busy} onClick={() => onRecur(recur)}>
            Make it recurring
          </button>
        </div>
      )}
    </div>
  );
}
