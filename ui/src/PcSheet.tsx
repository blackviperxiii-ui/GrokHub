import { useEffect, useRef, useState } from "react";
import { homePcDetails, type TileRow } from "./api";
import { useModalFocus } from "./ui";

export const PC_SHEET_TITLE = "This PC";
export const PC_SHEET_READING = "Reading this PC…";
/** How often the sheet looks again while the launch hardware read is still running. */
const REREAD_MS = 5_000;

/** Home's PC Loadout → Details: everything this PC's hardware read found,
 * one label and value per line. GPU names and drivers come from the OS, so
 * they show as plain text. */
export function PcSheet({ onClose }: { onClose: () => void }) {
  const [rows, setRows] = useState<TileRow[] | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const close = useRef<HTMLButtonElement>(null);
  const box = useRef<HTMLDivElement>(null);
  useModalFocus(box);

  useEffect(() => {
    let gone = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const read = () => {
      homePcDetails().then(
        (next) => {
          if (gone) return;
          if (next) setRows(next);
          else timer = setTimeout(read, REREAD_MS);
        },
        (err) => {
          if (!gone) setNote(String(err));
        },
      );
    };
    read();
    return () => {
      gone = true;
      clearTimeout(timer);
    };
  }, []);

  useEffect(() => {
    close.current?.focus();
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      // Esc belongs to a window on top of the sheet, such as the palette.
      const owner = (e.target as HTMLElement | null)?.closest?.("[role='dialog']");
      if (owner && owner !== box.current) return;
      onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div className="fix-backdrop" onClick={onClose}>
      <div
        ref={box}
        className="card fix-window"
        role="dialog"
        aria-modal="true"
        aria-label={PC_SHEET_TITLE}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="fix-head">
          <h2>{PC_SHEET_TITLE}</h2>
          <button ref={close} type="button" className="quiet" title="Close (Esc)" onClick={onClose}>
            Close
          </button>
        </div>
        {note ? (
          <p className="status" role="status">
            {note}
          </p>
        ) : rows ? (
          <dl className="pc-sheet">
            {rows.map((r, i) => (
              <div key={i}>
                <dt>{r.label}</dt>
                <dd>{r.value}</dd>
              </div>
            ))}
          </dl>
        ) : (
          <p className="empty">{PC_SHEET_READING}</p>
        )}
      </div>
    </div>
  );
}
