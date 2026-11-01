import { useState, type FormEvent } from "react";
import type { Approvals, ElicitAction, ElicitChoice, SessionMode } from "./api";
import { APPROVALS } from "./approvals";
import { PRODUCT_NAME } from "./brand";
import { ModelPicker } from "./ModelPicker";

/** The cabin's composer chips, each with its hover line. */
export const SESSION_CHIPS: { mode: SessionMode; label: string; tip: string }[] = [
  {
    mode: "chat",
    label: "Chat",
    tip: "Normal chat. Grok can edit files and use tools in your work folder. Everyday work.",
  },
  {
    mode: "plan",
    label: "Plan",
    tip: "Grok writes a plan before changing things, and changes nothing until you approve it. Use this for bigger or riskier work.",
  },
  {
    mode: "btw",
    label: "btw",
    tip: "A look-only side ask. Grok can read and answer, but changes nothing.",
  },
];

export const APPROVAL_CHIPS: { value: Approvals; label: string }[] = [
  { value: "ask", label: "Ask" },
  { value: "auto", label: "Auto" },
  { value: "always", label: "Always" },
];

export const EFFORT_TIP = `Auto effort: ${PRODUCT_NAME} picks how hard Grok thinks for each step.`;
export const REMOTE_APPROVALS_TIP = `Set in ${PRODUCT_NAME} on your computer.`;

export const REMOTE_MODEL_TIP = `The model is set in ${PRODUCT_NAME} on your computer.`;

/** Chat / Plan / btw, Ask / Auto / Always, the model and the read-only effort
 * chip. `approvals` and `model` are null until Settings loads. On the web app
 * the approvals and the model stay the computer's, so those chips only show it. */
export function ModeChips({
  mode,
  onMode,
  approvals,
  onApprovals,
  busy,
  remote,
  model = null,
  onModel,
}: {
  mode: SessionMode;
  onMode: (mode: SessionMode) => void;
  approvals: Approvals | null;
  onApprovals: (approvals: Approvals) => void;
  busy: boolean;
  remote: boolean;
  model?: string | null;
  onModel?: (id: string) => void;
}) {
  return (
    <div className="chips">
      <div className="chip-group" role="group" aria-label="Session mode">
        {SESSION_CHIPS.map((chip) => (
          <button
            key={chip.mode}
            type="button"
            className="chip"
            title={chip.tip}
            aria-pressed={mode === chip.mode}
            disabled={busy}
            onClick={() => onMode(chip.mode)}
          >
            {chip.label}
          </button>
        ))}
      </div>
      <div className="chip-group" role="group" aria-label="Approvals">
        {APPROVAL_CHIPS.map((chip) => (
          <button
            key={chip.value}
            type="button"
            className="chip"
            title={remote ? REMOTE_APPROVALS_TIP : APPROVALS[chip.value].note}
            aria-pressed={approvals === chip.value}
            disabled={busy || remote || approvals === null}
            onClick={() => onApprovals(chip.value)}
          >
            {chip.label}
          </button>
        ))}
      </div>
      {model !== null &&
        (remote || !onModel ? (
          <span className="chip quiet" title={REMOTE_MODEL_TIP}>
            Model: {model}
          </span>
        ) : (
          <ModelPicker value={model} onPick={onModel} disabled={busy} chip />
        ))}
      <span className="chip quiet" title={EFFORT_TIP}>
        Auto effort
      </span>
    </div>
  );
}

export type ElicitItem = {
  kind: "elicit";
  id: string;
  server: string;
  message: string;
  mode: "form" | "url";
  url: string;
  fieldName: string | null;
  fieldTitle: string;
  secret: boolean;
  options?: ElicitChoice[];
  /** "ended": the turn stopped before anyone answered. */
  answer: ElicitAction | "ended" | null;
};

const PLAN_ANSWERS: Record<ElicitAction | "ended", string> = {
  accept: "Approved. Grok can make the changes now.",
  decline: "Kept in plan mode.",
  cancel: "Cancelled.",
  ended: "Not answered: the turn ended.",
};

const ANSWERS: Record<ElicitAction | "ended", string> = {
  accept: "Sent.",
  decline: "Declined.",
  cancel: "Cancelled.",
  ended: "Not answered: the turn ended.",
};

/** A plan to approve, Grok's question, or a connector's form or link. */
export function ElicitCard({
  item,
  onAnswer,
}: {
  item: ElicitItem;
  onAnswer: (action: ElicitAction, value: string) => void;
}) {
  const [value, setValue] = useState("");
  const [picked, setPicked] = useState<string | null>(null);
  const plan = item.server === "Plan";
  const options = item.options ?? [];
  // The message lists the options as text for the cabin; here they are buttons.
  const message = options.length
    ? item.message
        .split("\n")
        .filter((line) => !line.startsWith("- ") && line !== "Reply with the option label.")
        .join("\n")
        .trim()
    : item.message;
  const title = plan
    ? "Grok's plan"
    : item.server === "Question"
      ? "Grok asks"
      : `${item.server} needs something from you`;

  function send(event: FormEvent) {
    event.preventDefault();
    if (item.fieldName && !value.trim()) return;
    onAnswer("accept", value);
  }

  return (
    <div className="card elicit" role="group" aria-label={title}>
      <div className="card-title">{title}</div>
      <pre className="card-detail plain">{message}</pre>
      {item.mode === "url" && item.url && <code className="card-detail-line">{item.url}</code>}
      {item.answer ? (
        <p className="card-answer">
          {item.answer === "accept" && picked
            ? `You chose: ${picked}`
            : (plan ? PLAN_ANSWERS : ANSWERS)[item.answer]}
        </p>
      ) : plan ? (
        <div className="card-buttons">
          <button type="button" onClick={() => onAnswer("accept", "")}>
            Approve plan
          </button>
          <button type="button" className="quiet" onClick={() => onAnswer("decline", "")}>
            Keep planning
          </button>
        </div>
      ) : (
        <>
          {options.length > 0 && (
            <div className="answer-options" role="group" aria-label="Options">
              {options.map((option) => (
                <button
                  key={option.label}
                  type="button"
                  className={option.recommended ? "choice" : "choice quiet"}
                  onClick={() => {
                    setPicked(option.label);
                    onAnswer("accept", option.label);
                  }}
                >
                  <span className="choice-label">
                    {option.label}
                    {option.recommended && (
                      <>
                        {" "}
                        <span className="badge">Recommended</span>
                      </>
                    )}
                  </span>
                  {option.description && (
                    <span className="choice-detail">{option.description}</span>
                  )}
                </button>
              ))}
            </div>
          )}
          <form className="card-buttons" onSubmit={send}>
            {item.fieldName && (
              <input
                aria-label={item.fieldTitle || "Answer"}
                placeholder={options.length ? "Or type your own answer" : item.fieldTitle || "Answer"}
                type={item.secret ? "password" : "text"}
                autoComplete="off"
                value={value}
                onChange={(e) => setValue(e.target.value)}
              />
            )}
            <button type="submit" disabled={Boolean(item.fieldName) && !value.trim()}>
              {item.mode === "url" ? "I've finished" : "Send"}
            </button>
            <button type="button" className="quiet" onClick={() => onAnswer("decline", "")}>
              Decline
            </button>
          </form>
        </>
      )}
    </div>
  );
}
