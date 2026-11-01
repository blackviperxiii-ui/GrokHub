import { useEffect, useRef, useState } from "react";
import { pulseFix, type FixKind, type FixView, type PulseVerdict } from "./api";
import { PRODUCT_NAME } from "./brand";

/** The note at the foot of the options window. */
export const READ_ONLY_NOTE = `${PRODUCT_NAME} only reads sensors. It never changes fans, clocks or drivers, and never touches a game.`;

/** A dot in the verdict's color and its one line. */
export function VerdictLine({ verdict }: { verdict: PulseVerdict | null }) {
  if (!verdict) return null;
  return (
    <p className={`verdict ${verdict.tone}`}>
      <span className="verdict-dot" aria-hidden="true" />
      {verdict.line}
    </p>
  );
}

/** What a row's button says, closed and open. */
function rowButton(kind: FixKind, open: boolean): string {
  if (kind === "compare") return open ? "Hide" : "Compare";
  if (kind === "ask") return "Ask";
  return open ? "Hide steps" : "Show steps";
}

/** A PC health card's options window: the verdict, what to try with its
 * steps, the last few sessions side by side, and Ask. Nothing here changes
 * the PC; Ask hands the card to chat. */
export function FixWindow({
  id,
  label,
  onAsk,
  onClose,
}: {
  id: string;
  label: string;
  onAsk: () => void;
  onClose: () => void;
}) {
  const [view, setView] = useState<FixView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<FixKind | null>(null);
  const close = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    pulseFix(id).then(setView, (err) => setError(String(err)));
  }, [id]);

  useEffect(() => {
    close.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="fix-backdrop" onClick={onClose}>
      <div
        className="card fix-window"
        role="dialog"
        aria-modal="true"
        aria-label={label}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="fix-head">
          <h2>{label}</h2>
          <button ref={close} type="button" className="quiet icon-button" aria-label="Close" onClick={onClose}>
            ×
          </button>
        </div>
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
        {view && (
          <>
            <div className="fix-title">{view.title}</div>
            <VerdictLine verdict={view.verdict} />
            <ul className="fix-rows">
              {view.options.map((o) => {
                const shown = open === o.kind;
                return (
                  <li key={o.kind} className="fix-row" aria-label={o.title}>
                    <div className="fix-row-head">
                      <div className="fix-row-text">
                        <span className="fix-row-title">{o.title}</span>
                        <span className="fix-row-why">{o.why}</span>
                      </div>
                      <button
                        type="button"
                        className={o.kind === "ask" ? undefined : "quiet"}
                        aria-expanded={o.kind === "ask" ? undefined : shown}
                        onClick={() => {
                          if (o.kind === "ask") onAsk();
                          else setOpen(shown ? null : o.kind);
                        }}
                      >
                        {rowButton(o.kind, shown)}
                      </button>
                    </div>
                    {shown && o.kind === "compare" && (
                      <table className="fix-compare">
                        <thead>
                          <tr>
                            {["Session", "Time", "GPU peak", "CPU peak", "GPU busy"].map((h) => (
                              <th key={h}>{h}</th>
                            ))}
                          </tr>
                        </thead>
                        <tbody>
                          {view.compare.map((r, i) => (
                            <tr key={i}>
                              <td>{r.session}</td>
                              <td>{r.time}</td>
                              <td>{r.gpuPeak}</td>
                              <td>{r.cpuPeak}</td>
                              <td>{r.gpuBusy}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    )}
                    {shown && o.steps.length > 0 && (
                      <ol className="fix-steps">
                        {o.steps.map((s, i) => (
                          <li key={i}>{s}</li>
                        ))}
                      </ol>
                    )}
                  </li>
                );
              })}
            </ul>
            <p className="fix-note">{READ_ONLY_NOTE}</p>
          </>
        )}
      </div>
    </div>
  );
}
