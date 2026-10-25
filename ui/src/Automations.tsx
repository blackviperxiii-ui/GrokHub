import { useEffect, useState, type FormEvent } from "react";
import {
  addAutomation,
  listAutomations,
  removeAutomation,
  runAutomation,
  setAutomationEnabled,
  type AutomationRow,
  type RunResult,
} from "./api";

function resultLine(result: RunResult): string {
  if (result.error) return result.error;
  return result.text.trim() || "Finished with no reply.";
}

/** `active`: the tab is showing. The page stays mounted so a Run now in
 * flight keeps its button and its reply; it reloads the list on each visit. */
export const AUTOMATIONS_EMPTY =
  "No automations yet. Describe one above in plain words and it runs on its own schedule.";

export function Automations({ active = true }: { active?: boolean }) {
  const [rows, setRows] = useState<AutomationRow[] | null>(null);
  const [draft, setDraft] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [running, setRunning] = useState<string | null>(null);
  const [results, setResults] = useState<Record<string, string>>({});

  useEffect(() => {
    if (active) listAutomations().then(setRows, (err) => setNote(String(err)));
  }, [active]);

  async function act(step: () => Promise<AutomationRow[]>) {
    try {
      setRows(await step());
      setNote(null);
    } catch (err) {
      setNote(String(err));
    }
  }

  async function add(event: FormEvent) {
    event.preventDefault();
    const text = draft.trim();
    if (!text) return;
    try {
      setRows(await addAutomation(text));
      setDraft("");
      setNote(null);
    } catch (err) {
      setNote(String(err));
    }
  }

  async function run(id: string) {
    setRunning(id);
    try {
      const result = await runAutomation(id);
      setResults((now) => ({ ...now, [id]: resultLine(result) }));
      setRows(await listAutomations());
    } catch (err) {
      setResults((now) => ({ ...now, [id]: String(err) }));
    } finally {
      setRunning(null);
    }
  }

  return (
    <section className="automations" aria-label="Automations">
      <form className="add" onSubmit={add}>
        <input
          aria-label="New automation"
          placeholder="every weekday at 9am summarize my inbox"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
        />
        <button type="submit" disabled={!draft.trim()}>
          Add
        </button>
      </form>
      <p className="note">
        The GrokHub cabin runs these on schedule. Run now works here: it can read, and anything
        that would need your OK is skipped.
      </p>
      {note && (
        <p className="error" role="alert">
          {note}
        </p>
      )}
      {rows === null && !note && <p className="empty">Loading automations…</p>}
      {rows?.length === 0 && <p className="empty">{AUTOMATIONS_EMPTY}</p>}
      <ul className="rows">
        {rows?.map((row) => (
          <li key={row.id} className="card automation" aria-label={row.name}>
            <div className="card-title">{row.name}</div>
            <div className="card-status">{row.summary}</div>
            {row.health && <div className="card-health">{row.health}</div>}
            {results[row.id] && <pre className="card-detail">{results[row.id]}</pre>}
            <div className="card-buttons">
              <button
                type="button"
                disabled={running !== null}
                onClick={() => void run(row.id)}
              >
                {running === row.id ? "Running…" : "Run now"}
              </button>
              <button
                type="button"
                className="quiet"
                onClick={() => void act(() => setAutomationEnabled(row.id, !row.enabled))}
              >
                {row.enabled ? "Pause" : "Resume"}
              </button>
              <button
                type="button"
                className="quiet danger"
                onClick={() => void act(() => removeAutomation(row.id))}
              >
                Remove
              </button>
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}
