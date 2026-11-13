import { Markdown } from "./Markdown";
import { displayText } from "./md";

/** A side answer runs under `<chat>~side-<n>`; its events carry that id
 * (src-tauri/src/turns.rs). */
export const SIDE_MARK = "~side-";

/** A question typed while a reply runs, answered beside it. */
export interface Side {
  id: string;
  question: string;
  answer: string;
  live: boolean;
  failed: boolean;
}

export const SIDE_LABEL = "On the side";
export const SIDE_ANSWERING = "Answering…";
export const SIDE_FAILED = "Couldn't answer that on the side.";
export const SIDE_STOPPED = "Stopped.";
/** Finished side answers kept on screen; older ones drop off. */
export const SIDE_KEEP = 3;

/** Adds `side`, keeping every live one and the newest finished ones. */
export function withSide(sides: readonly Side[], side: Side): Side[] {
  const all = [...sides, side];
  let finished = all.filter((s) => !s.live).length;
  return all.filter((s) => {
    if (s.live || finished <= SIDE_KEEP) return true;
    finished -= 1;
    return false;
  });
}

/** The "On the side" cards above the box: each answer, with Stop while it
 * runs, then Tell the main task (feeds it to the work) and Dismiss. The
 * answers never join the chat or its history. */
export function SideCards({
  sides,
  onStop,
  onTell,
  onDismiss,
}: {
  sides: readonly Side[];
  onStop: (side: Side) => void;
  onTell: (side: Side) => void;
  onDismiss: (side: Side) => void;
}) {
  if (sides.length === 0) return null;
  return (
    <div className="side-cards" role="region" aria-label={SIDE_LABEL}>
      {sides.map((side) => {
        const empty = !side.answer.trim();
        return (
          <div key={side.id} className="card side-card" role="group" aria-label={`${SIDE_LABEL}: ${side.question}`}>
            <div className="card-title">
              <span className="side-label">{SIDE_LABEL}</span> <span className="side-question">{side.question}</span>
            </div>
            {empty ? (
              <p className="card-reason">
                {side.live ? SIDE_ANSWERING : side.failed ? SIDE_FAILED : SIDE_STOPPED}
              </p>
            ) : (
              <div className="side-answer">
                <Markdown text={displayText(side.answer)} />
              </div>
            )}
            <div className="card-buttons">
              {side.live ? (
                <button type="button" className="quiet" onClick={() => onStop(side)}>
                  Stop
                </button>
              ) : (
                <>
                  {!empty && (
                    <button type="button" onClick={() => onTell(side)}>
                      Tell the main task
                    </button>
                  )}
                  <button type="button" className="quiet" onClick={() => onDismiss(side)}>
                    Dismiss
                  </button>
                </>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}
