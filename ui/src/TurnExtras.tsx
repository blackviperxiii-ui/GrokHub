import { useState } from "react";
import type { ChatEvent } from "./api";

type FindingsEvent = Extract<ChatEvent, { kind: "findings" }>;
type RecapEvent = Extract<ChatEvent, { kind: "recap" }>;
type EvidenceEvent = Extract<ChatEvent, { kind: "evidence" }>;

/** A `report_findings` card (Order 733): what the run found, with a pill
 * per fix. A pill puts its fix in the composer — nothing runs until it is
 * sent, so approvals still apply. Dismiss sets the card aside, as the
 * cabin's ghost pill does. */
export function FindingsCard({
  event,
  onFix,
}: {
  event: FindingsEvent;
  /** Puts a fix's prompt in the composer. */
  onFix: (text: string) => void;
}) {
  const [dismissed, setDismissed] = useState(false);
  if (dismissed) return null;
  return (
    <div className="card turn-extra findings" role="group" aria-label="Findings from this run">
      <div className="card-title">Findings from this run</div>
      <ul className="finding-items">
        {event.items.map((item, i) => (
          <li key={i} className="finding-item">
            <span className={`severity severity-${item.severity}`}>{item.severity}</span>
            <span>{item.text}</span>
          </li>
        ))}
      </ul>
      <div className="fix-pills">
        {event.doAll && (
          <button type="button" className="chip fix-pill" onClick={() => onFix(event.doAll!.join("\n"))}>
            Do all
          </button>
        )}
        {event.fixes.map((fix) => (
          <button
            key={fix.label}
            type="button"
            className="chip fix-pill"
            onClick={() => onFix(fix.prompt)}
          >
            {fix.label}
          </button>
        ))}
        <button type="button" className="chip fix-pill quiet" onClick={() => setDismissed(true)}>
          Dismiss
        </button>
      </div>
    </div>
  );
}

const RECAP_MARK: Record<RecapEvent["lines"][number]["status"], string> = {
  done: "✓",
  partly: "!",
  notDone: "×",
};

/** The reply's `Finished` block as a card (Order 733): the tally, one line
 * per task with its mark, what still needs you, and the turn's tool steps
 * behind a fold. The block itself leaves the reply bubble (see Chat). */
export function RecapCard({ event }: { event: RecapEvent }) {
  const [open, setOpen] = useState(false);
  const done = event.lines.filter((line) => line.status === "done").length;
  return (
    <div className="card turn-extra recap" role="group" aria-label="Finished">
      <div className="card-title">
        FINISHED
        <span className="recap-tally">
          {done} of {event.lines.length} done
        </span>
      </div>
      <ul className="recap-lines">
        {event.lines.map((line, i) => (
          <li key={i} className={`recap-line ${line.status}`}>
            <span className="recap-mark" aria-hidden="true">
              {RECAP_MARK[line.status]}
            </span>
            <span>{line.text}</span>
          </li>
        ))}
        {event.needsYou.map((text, i) => (
          <li key={`need-${i}`} className="recap-line needs-you">
            <span className="recap-mark" aria-hidden="true">
              →
            </span>
            <span>{text}</span>
          </li>
        ))}
        {event.undo.map((text, i) => (
          <li key={`undo-${i}`} className="recap-line undo">
            <span className="recap-mark" aria-hidden="true">
              ↺
            </span>
            <span>{text}</span>
          </li>
        ))}
      </ul>
      {event.steps.length > 0 && (
        <>
          <button
            type="button"
            className="link recap-fold"
            aria-expanded={open}
            onClick={() => setOpen((o) => !o)}
          >
            {open ? "Hide details" : "Show details"}
          </button>
          {open && (
            <ul className="recap-steps">
              {event.steps.map((step, i) => (
                <li key={i} className={step.failed ? "recap-step failed" : "recap-step"}>
                  <span className="recap-mark" aria-hidden="true">
                    {step.failed ? "×" : "·"}
                  </span>
                  <span>{step.title}</span>
                </li>
              ))}
            </ul>
          )}
        </>
      )}
    </div>
  );
}

function EvidenceSection({ title, lines }: { title: string; lines: string[] }) {
  if (lines.length === 0) return null;
  return (
    <div className="evidence-section">
      <div className="evidence-title">{title}</div>
      <ul>
        {lines.map((line, i) => (
          <li key={i}>{line}</li>
        ))}
      </ul>
    </div>
  );
}

/** The evidence receipt under a finished turn (Order 733): a compact proof
 * of what the harness recorded. Closed by default, like the cabin's. */
export function EvidenceCard({ event }: { event: EvidenceEvent }) {
  const [open, setOpen] = useState(false);
  return (
    <div className="card turn-extra evidence" role="group" aria-label="Evidence">
      <button
        type="button"
        className="evidence-head"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="card-title">{event.summary}</span>
        <span className="evidence-toggle" aria-hidden="true">
          {open ? "▾" : "▸"}
        </span>
      </button>
      {open && (
        <div className="evidence-body">
          <EvidenceSection title="Goal" lines={[event.goal, event.goalDetail].filter(Boolean)} />
          <EvidenceSection title="Plan status" lines={event.plan} />
          <EvidenceSection title="Actions" lines={event.actions} />
          <EvidenceSection title="Checks" lines={event.checks} />
          <EvidenceSection title="Changes" lines={event.changes} />
          <EvidenceSection title="Confidence" lines={[event.confidence]} />
          <EvidenceSection title="Recovery" lines={[event.recovery]} />
          {event.toolRanNotVerified && (
            <p className="evidence-note">A successful tool is not proof the goal succeeded.</p>
          )}
        </div>
      )}
    </div>
  );
}
