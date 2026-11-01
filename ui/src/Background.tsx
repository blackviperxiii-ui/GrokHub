import { useEffect, useState } from "react";
import { getBackground, type BackgroundPage } from "./api";

/** Settings → Background: who fires automations, the next one due, and when
 * each last ran, from the automations.json the cabin shares. Read-only, so
 * the web app sees the same page. It reads again when the window gets focus. */
export function Background() {
  const [page, setPage] = useState<BackgroundPage | null>(null);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    const load = () => getBackground().then(setPage, (err) => setNote(String(err)));
    void load();
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, []);

  if (!page) return <p className="note">{note ?? "Loading…"}</p>;

  return (
    <div className="background" aria-label="Background">
      <p className="note">{page.runner}</p>
      <p>{page.nextDue ? `Next due: ${page.nextDue}` : "No automation is on, so nothing is due."}</p>
      <p className="note">
        {page.quiet} · {page.budget}
      </p>
      {page.rows.length > 0 && (
        <ul className="background-rows" aria-label="Automations">
          {page.rows.map((row) => (
            <li key={row.id}>
              <strong>{row.name}</strong>
              <span className="note">
                {row.next} · {row.last}
              </span>
              {row.health && <span className="note warn">{row.health}</span>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
