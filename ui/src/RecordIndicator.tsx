// "Record my screen" and "Check my audio": the top bar's always-visible
// indicator. While a recording runs it shows the red clock and a Stop button
// and never hides; the audio check shows its own line while the microphone is
// open. Finished runs go to the notice bell once.
import { useEffect, useEffectEvent, useState } from "react";
import { recordStatus, recordStop, type RecordEvent, type RecordStatus } from "./api";

export const RECORD_POLL_MS = 1000;
export const STOP_TITLE = "Stop the screen recording (or send /record stop)";

export function RecordIndicator({
  onEvent,
  poll = recordStatus,
  stop = recordStop,
  everyMs = RECORD_POLL_MS,
}: {
  /** A recording or an audio check just finished. */
  onEvent: (event: RecordEvent) => void;
  poll?: () => Promise<RecordStatus>;
  stop?: () => Promise<boolean>;
  everyMs?: number;
}) {
  const [status, setStatus] = useState<RecordStatus | null>(null);
  const [stopping, setStopping] = useState(false);
  const handle = useEffectEvent(onEvent);

  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const tick = async () => {
      try {
        const next = await poll();
        if (!live) return;
        setStatus(next);
        if (!next.recording) setStopping(false);
        for (const event of next.events) handle(event);
      } catch {
        // The shell is closing, or this build has no recorder: say nothing.
      }
      if (live) timer = setTimeout(tick, everyMs);
    };
    void tick();
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [poll, everyMs]);

  if (!status || (!status.recording && !status.listening)) return null;
  return (
    <div className="record-indicator" role="status" aria-live="polite">
      {status.recording && (
        <>
          <span className="record-line">{status.recording.text}</span>
          <button
            type="button"
            className="record-stop"
            title={STOP_TITLE}
            disabled={stopping}
            onClick={() => {
              setStopping(true);
              void stop().catch(() => setStopping(false));
            }}
          >
            {stopping ? "Stopping…" : "Stop"}
          </button>
        </>
      )}
      {status.listening && <span className="record-line">{status.listening.text}</span>}
    </div>
  );
}
