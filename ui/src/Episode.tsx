// The supervised desktop episode panel: the step list, the latest screen as a
// thumbnail (never while a credentials step waits), the cards for steps that
// wait for an OK, and Pause, Stop and Halt. It follows `episode://event`.
// An Allow with no reply running goes on at once: the chat sends the resume.
import { useEffect, useEffectEvent, useState } from "react";
import { haltDesktop } from "./api";
import {
  episodeAnswer,
  episodeArm,
  episodeArmed,
  episodePause,
  episodeState,
  episodeStop,
  onEpisode,
  type EpisodePanel,
  type EpisodeStep,
  type EpisodePark,
} from "./episodeApi";

export const WAITING_NOTE = "This step waits for your OK, even in Always.";
export const ARM_LABEL = "Supervised desktop session";
export const ARM_NOTE =
  "Messages run as one watched session: you see each step, and risky steps wait for your OK.";
export const CUA_NOTE = "Using the Cua driver";
/** What the chat sends to go on after an Allow (the shell's `RESUME_TEXT`):
 * the session keeps its goal and runs the allowed step now. */
export const RESUME_TEXT = "Continue";

/** The label of the part of a card that says what the step does. */
export const CARD_ACTION = "What this step does";
/** The kind of step a card waits on, by the shell's hard class. */
export const CLASS_WORDS: Record<string, string> = {
  send: "Send",
  delete: "Delete",
  money: "Spend money",
  credentials: "Password or key",
  irreversible_os: "Can't be undone",
  system_admin: "Admin rights or system protection",
  agent_config: "Agent instructions and settings",
  automation: "New automation",
  outside_value: "Value from outside text",
  runs_later: "File that runs code later",
  flagged_driver: "Driver anti-cheats block",
};

export function classWords(cls: string): string {
  return CLASS_WORDS[cls] ?? "Needs your OK";
}

/** A panel as the shell sends it. `frameSame`: the picture is the one the
 * page already has, so it was left out. `resume`: a reply ended with an
 * allowed step nobody ran, so the chat sends the resume now. */
type SentPanel = EpisodePanel & { frameSame?: boolean; resume?: boolean };

/** The panel to draw for `next`, with the picture kept when the shell left
 * it out as the same one. */
function withFrame(next: SentPanel, had: SentPanel | null): SentPanel {
  if (!next.frameSame || !had || had.episodeId !== next.episodeId || had.frame === null) return next;
  return { ...next, frame: had.frame, frameSame: false };
}
export const ENDED_WORDS: Record<string, string> = {
  verified: "Done, and checked.",
  halt: "Halted. Nothing more ran.",
  stop: "Stopped. Nothing more ran.",
  idle: "Ended after sitting idle.",
  unconfirmed: "Ended: the check couldn't confirm it.",
};

export function endedLine(ended: string): string {
  return ENDED_WORDS[ended] ?? "Ended.";
}

/** One step's outcome as words a person reads. A step still parked, or
 * allowed and never picked up, when the session is over never ran. */
export function stepWords(step: EpisodeStep, over: boolean): string {
  switch (step.decision) {
    case "allow":
    case "approve":
      return step.result ? `ran: ${step.result}` : "ran";
    case "park":
      return over ? "not run" : "waiting for your OK";
    case "allowed":
      return over ? "not run" : "allowed, not run yet";
    case "deny":
      return step.result ? `not run: ${step.result}` : "not run";
    default:
      return step.decision;
  }
}

function ParkCard({ park, onAnswer }: { park: EpisodePark; onAnswer: (approve: boolean) => void }) {
  return (
    <div className="episode-park" role="group" aria-label={`Waiting: ${park.action}`}>
      <p className="episode-park-class">{classWords(park.class)}</p>
      {/* All of it, lines kept: it wraps, and scrolls (by keyboard too) when long. */}
      <div className="episode-park-action" role="region" aria-label={CARD_ACTION} tabIndex={0}>
        {park.action}
      </div>
      <p className="note">{WAITING_NOTE}</p>
      <div className="episode-actions">
        <button type="button" onClick={() => onAnswer(true)}>
          Allow
        </button>
        <button type="button" className="link" onClick={() => onAnswer(false)}>
          Deny
        </button>
      </div>
    </div>
  );
}

export function EpisodeView({
  panel,
  onAnswer,
  onPause,
  onStop,
  onHalt,
  onDismiss,
}: {
  panel: EpisodePanel;
  onAnswer: (parkId: string, approve: boolean) => void;
  onPause: () => void;
  onStop: () => void;
  onHalt: () => void;
  onDismiss: () => void;
}) {
  const over = panel.state === "ended";
  return (
    <section className="episode" aria-label="Desktop session">
      <header className="episode-head">
        <h3>{panel.header}</h3>
        {panel.state === "paused" && <span className="episode-state">Paused</span>}
        {panel.cua && <span className="episode-state">{CUA_NOTE}</span>}
      </header>
      {panel.frame && <img className="episode-frame" src={panel.frame} alt="The latest screen" />}
      {panel.steps.length > 0 && (
        <ol className="episode-steps" aria-label="Steps">
          {panel.steps.map((s) => (
            <li key={s.n} data-decision={s.decision}>
              <span className="episode-n">{s.n}</span>
              <span className="episode-tool">{s.tool}</span>
              <span className="episode-result">{stepWords(s, over)}</span>
            </li>
          ))}
        </ol>
      )}
      {panel.parks.map((p) => (
        <ParkCard key={p.id} park={p} onAnswer={(approve) => onAnswer(p.id, approve)} />
      ))}
      {over ? (
        <div className="episode-actions">
          <p className="note" role="status">
            {endedLine(panel.ended ?? "")}
          </p>
          <button type="button" className="link" onClick={onDismiss}>
            Dismiss
          </button>
        </div>
      ) : (
        <div className="episode-actions">
          {panel.state === "running" && (
            <button type="button" onClick={onPause}>
              Pause
            </button>
          )}
          <button type="button" onClick={onStop}>
            Stop
          </button>
          <button type="button" className="halt" onClick={onHalt}>
            Halt
          </button>
        </div>
      )}
    </section>
  );
}

/** The switch that starts a supervised desktop session on this chat. */
export function ArmSwitch({ on, onChange }: { on: boolean; onChange: (on: boolean) => void }) {
  return (
    <label className="episode-arm">
      <input type="checkbox" checked={on} onChange={(e) => onChange(e.target.checked)} />
      <span>{ARM_LABEL}</span>
      <span className="note">{ARM_NOTE}</span>
    </label>
  );
}

/** The chat's supervised desktop session: its switch (while desktop control
 * is on) and, once one is open, its panel above the composer. Nothing
 * outside the desktop app. `onResume` sends a message through the chat. */
export function Episode({
  conversationId,
  desktopOn,
  onResume,
}: {
  conversationId: string;
  desktopOn: boolean;
  onResume?: (text: string) => void;
}) {
  const [panel, setPanel] = useState<SentPanel | null>(null);
  const [armed, setArmed] = useState(false);
  const [failed, setFailed] = useState("");
  const resume = useEffectEvent(() => onResume?.(RESUME_TEXT));

  useEffect(() => {
    let dropped = false;
    let stop: (() => void) | undefined;
    void onEpisode((next: SentPanel) => {
      if (next.conversationId !== conversationId) return;
      setPanel((had) => withFrame(next, had));
      if (next.resume) resume();
    }).then((unlisten) => {
      if (dropped) unlisten();
      else stop = unlisten;
    });
    episodeState(conversationId)
      .then((now) => {
        if (dropped || !now) return;
        // An event that came first may have left the picture out as the same one.
        setPanel((had) => {
          if (!had) return now;
          return had.frameSame && had.frame === null ? { ...had, frame: now.frame, frameSame: false } : had;
        });
      })
      .catch(() => {});
    episodeArmed(conversationId)
      .then((on) => {
        if (!dropped) setArmed(on);
      })
      .catch(() => {});
    return () => {
      dropped = true;
      stop?.();
    };
  }, [conversationId]);

  const run = (did: Promise<unknown>) => {
    setFailed("");
    did.catch((e: unknown) => setFailed(String(e)));
  };

  // The shell answers true when the allowed step waits on a resume: no reply
  // is running to pick the answer up, so the chat sends it now.
  const answer = (parkId: string, approve: boolean) => {
    setFailed("");
    episodeAnswer(conversationId, parkId, approve)
      .then((due: unknown) => {
        if (due === true) onResume?.(RESUME_TEXT);
      })
      .catch((e: unknown) => setFailed(String(e)));
  };

  const arm = (on: boolean) => {
    setFailed("");
    episodeArm(conversationId, on)
      .then(setArmed)
      .catch((e: unknown) => setFailed(String(e)));
  };

  if (!panel && !desktopOn) return null;
  return (
    <>
      {desktopOn && !panel && <ArmSwitch on={armed} onChange={arm} />}
      {panel && (
        <EpisodeView
          panel={panel}
          onAnswer={answer}
          onPause={() => run(episodePause(conversationId))}
          onStop={() => run(episodeStop(conversationId))}
          onHalt={() => run(haltDesktop())}
          onDismiss={() => {
            setPanel(null);
            setArmed(false);
          }}
        />
      )}
      {failed && (
        <p className="error" role="alert">
          {failed}
        </p>
      )}
    </>
  );
}
