// The supervised desktop episode panel: the step list, the latest screen as a
// thumbnail (never while a credentials step waits), the cards for steps that
// wait for an OK, and Pause, Stop and Halt. It follows `episode://event`.
import { useEffect, useState } from "react";
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

/** One step's outcome as words a person reads. A step still parked when the
 * session is over never ran. */
export function stepWords(step: EpisodeStep, over: boolean): string {
  switch (step.decision) {
    case "allow":
      return step.result ? `ran: ${step.result}` : "ran";
    case "park":
      return over ? "not run" : "waiting for your OK";
    case "deny":
      return step.result ? `not run: ${step.result}` : "not run";
    default:
      return step.decision;
  }
}

function ParkCard({ park, onAnswer }: { park: EpisodePark; onAnswer: (approve: boolean) => void }) {
  return (
    <div className="episode-park" role="group" aria-label={`Waiting: ${park.action}`}>
      <p className="episode-park-action">{park.action}</p>
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
 * outside the desktop app. */
export function Episode({ conversationId, desktopOn }: { conversationId: string; desktopOn: boolean }) {
  const [panel, setPanel] = useState<EpisodePanel | null>(null);
  const [armed, setArmed] = useState(false);
  const [failed, setFailed] = useState("");

  useEffect(() => {
    let dropped = false;
    let stop: (() => void) | undefined;
    void onEpisode((next) => {
      if (next.conversationId === conversationId) setPanel(next);
    }).then((unlisten) => {
      if (dropped) unlisten();
      else stop = unlisten;
    });
    episodeState(conversationId)
      .then((now) => {
        if (!dropped && now) setPanel((had) => had ?? now);
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
          onAnswer={(parkId, approve) => run(episodeAnswer(conversationId, parkId, approve))}
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
