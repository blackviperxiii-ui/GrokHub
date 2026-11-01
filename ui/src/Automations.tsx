import { useEffect, useState, type FormEvent } from "react";
import {
  addAutomation,
  editAutomation,
  listAutomations,
  onAutomationRan,
  removeAutomation,
  runAutomation,
  setAutomationEnabled,
  type AutomationRow,
  type RunResult,
} from "./api";
import { PRODUCT_NAME } from "./brand";
import { ConfirmRow } from "./ui";

export function resultLine(result: RunResult): string {
  if (result.error) return result.error;
  return result.text.trim() || "Finished with no reply.";
}

export const AUTOMATIONS_EMPTY =
  "No automations yet. Describe one above in plain words and it runs on its own schedule.";

/** A finished run, for the notice in chat and the Activity Dock. */
export type RunNotice = { id: string; name: string; ok: boolean; text: string };

/** `active`: the tab is showing. The page stays mounted so a Run now in
 * flight keeps its button and its reply; it reloads the list on each visit.
 * `onStarted` and `onFinished` hear about each run, so chat and the
 * Activity Dock can say so. */
export function Automations({
  active = true,
  onStarted,
  onFinished,
}: {
  active?: boolean;
  onStarted?: (id: string, name: string) => void;
  onFinished?: (notice: RunNotice) => void;
}) {
  const [rows, setRows] = useState<AutomationRow[] | null>(null);
  const [draft, setDraft] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [running, setRunning] = useState<string | null>(null);
  const [results, setResults] = useState<Record<string, string>>({});
  const [editing, setEditing] = useState<{ id: string; text: string } | null>(null);
  const [asking, setAsking] = useState<string | null>(null);

  useEffect(() => {
    if (active) listAutomations().then(setRows, (err) => setNote(String(err)));
  }, [active]);

  // A scheduled run the shell fired: show its reply and the row's new count.
  useEffect(() => {
    if (!active) return;
    let stop: (() => void) | undefined;
    let dropped = false;
    void onAutomationRan(({ id, result }) => {
      setResults((now) => ({ ...now, [id]: resultLine(result) }));
      listAutomations().then(setRows, (err) => setNote(String(err)));
    }).then((unlisten) => {
      if (dropped) unlisten();
      else stop = unlisten;
    });
    return () => {
      dropped = true;
      stop?.();
    };
  }, [active]);

  async function saveEdit(event: FormEvent) {
    event.preventDefault();
    if (!editing) return;
    try {
      setRows(await editAutomation(editing.id, editing.text.trim()));
      setEditing(null);
      setNote(null);
    } catch (err) {
      setNote(String(err));
    }
  }

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
    const name = rows?.find((row) => row.id === id)?.name ?? "Automation";
    setRunning(id);
    onStarted?.(id, name);
    try {
      const result = await runAutomation(id);
      setResults((now) => ({ ...now, [id]: resultLine(result) }));
      onFinished?.({ id, name, ok: !result.error, text: resultLine(result) });
      setRows(await listAutomations());
    } catch (err) {
      setResults((now) => ({ ...now, [id]: String(err) }));
      onFinished?.({ id, name, ok: false, text: String(err) });
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
        {PRODUCT_NAME} runs these on schedule, from the tray too. While the {PRODUCT_NAME} cabin is open,
        the cabin runs them. A run can read, and anything that would need your OK is skipped.
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
            <div className="card-status">
              {row.summary}
              {row.cabinOnly && (
                <>
                  {" "}
                  <span className="badge">Runs in the cabin</span>
                </>
              )}
            </div>
            {row.health && <div className="card-health">{row.health}</div>}
            {results[row.id] && <pre className="card-detail">{results[row.id]}</pre>}
            {editing?.id === row.id && (
              <form className="add" onSubmit={saveEdit}>
                <input
                  aria-label={`Edit ${row.name}`}
                  value={editing.text}
                  onChange={(e) => setEditing({ id: row.id, text: e.target.value })}
                />
                <button type="submit" disabled={!editing.text.trim()}>
                  Save
                </button>
                <button type="button" className="quiet" onClick={() => setEditing(null)}>
                  Cancel
                </button>
              </form>
            )}
            {asking === row.id ? (
              <ConfirmRow
                question={`Remove ${row.name}?`}
                detail="It won't run again."
                action="Remove"
                onConfirm={() => {
                  setAsking(null);
                  void act(() => removeAutomation(row.id));
                }}
                onCancel={() => setAsking(null)}
              />
            ) : (
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
              {row.describe !== null && (
                <button
                  type="button"
                  className="quiet"
                  disabled={editing?.id === row.id}
                  onClick={() => setEditing({ id: row.id, text: row.describe ?? "" })}
                >
                  Edit
                </button>
              )}
              <button
                type="button"
                className="quiet danger"
                onClick={() => setAsking(row.id)}
              >
                Remove…
              </button>
            </div>
            )}
          </li>
        ))}
      </ul>
    </section>
  );
}
