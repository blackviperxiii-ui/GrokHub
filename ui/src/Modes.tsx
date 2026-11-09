import { useState, type FormEvent } from "react";
import type { ElicitAction, ElicitChoice } from "./api";
import { externalUrlOk } from "./external";
import { PRODUCT_NAME } from "./brand";
import { ExternalLink } from "./ui";

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
  /** "ended": the turn stopped before anyone answered; "halted": Halt stopped it. */
  answer: ElicitAction | "ended" | "halted" | null;
  /** A helper asked; it waits past the end of the reply. */
  helper?: boolean;
};

const PLAN_ANSWERS: Record<ElicitAction | "ended" | "halted", string> = {
  accept: `Approved. ${PRODUCT_NAME} can make the changes now.`,
  decline: "Kept in plan mode.",
  cancel: "Cancelled.",
  ended: "Not answered: the turn ended.",
  halted: "Not answered: halted.",
};

const ANSWERS: Record<ElicitAction | "ended" | "halted", string> = {
  accept: "Sent.",
  decline: "Declined.",
  cancel: "Cancelled.",
  ended: "Not answered: the turn ended.",
  halted: "Not answered: halted.",
};

/** A plan to approve, the agent's question, or a connector's form or link. */
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
    ? "The plan"
    : item.server === "Question"
      ? `${PRODUCT_NAME} asks`
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
      {item.mode === "url" && item.url && (
        <p className="card-detail-line">
          {externalUrlOk(item.url) ? (
            <ExternalLink href={item.url}>Open {item.url}</ExternalLink>
          ) : (
            <code>{item.url}</code>
          )}
        </p>
      )}
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
