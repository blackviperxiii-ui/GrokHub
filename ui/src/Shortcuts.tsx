import { useEffect, useRef } from "react";

/** The shortcut sheet (Ctrl+/ or the palette's Shortcuts row): the shell's
 * keys, one per line. Escape, Close or a click outside closes it. */
export function Shortcuts({
  keys,
  onClose,
}: {
  keys: readonly { keys: string; action: string }[];
  onClose: () => void;
}) {
  const close = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    close.current?.focus();
  }, []);

  return (
    <div className="palette-backdrop" onMouseDown={onClose}>
      <div
        className="palette shortcuts"
        role="dialog"
        aria-modal="true"
        aria-label="Keyboard shortcuts"
        onMouseDown={(e) => e.stopPropagation()}
        onKeyDown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            onClose();
          }
        }}
      >
        <h2>Keyboard shortcuts</h2>
        <table>
          <tbody>
            {keys.map((row) => (
              <tr key={row.keys}>
                <th scope="row">
                  <kbd>{row.keys}</kbd>
                </th>
                <td>{row.action}</td>
              </tr>
            ))}
          </tbody>
        </table>
        <button ref={close} type="button" className="quiet" onClick={onClose}>
          Close
        </button>
      </div>
    </div>
  );
}
