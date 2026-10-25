import { useEffect, useState, type FormEvent } from "react";
import {
  duplicatePlaybook,
  getPlaybooks,
  resetPlaybook,
  savePlaybook,
  type PlaybookPage,
  type PlaybookRow,
} from "./api";

export const PLAYBOOKS_NOTE =
  "How the agent works through a task: look first, then safe steps, then risky ones, with a backup, a check and a way back. Risky steps still ask you first; a playbook never changes an approval.";
export const EDITOR_NOTE =
  "Saved as playbooks/<id>.toml in GrokHub's settings folder. Steps go look-only, then safe, then risky. A step that deletes, sends, pays, touches sign-ins or can't be undone must be risky, and commands GrokHub never runs are refused.";

const SOURCE_LABEL: Record<PlaybookRow["source"], string> = {
  shipped: "Built in",
  edited: "Edited",
  yours: "Yours",
};

/** Settings → Playbooks: the shipped ones with yours on top, from the same
 * files the cabin and the agent read. */
export function Playbooks() {
  const [page, setPage] = useState<PlaybookPage | null>(null);
  const [draft, setDraft] = useState<{ id: string; name: string; text: string } | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getPlaybooks().then(setPage, (err) => setNote(String(err)));
  }, []);

  function open(row: PlaybookRow) {
    setDraft({ id: row.id, name: row.name, text: row.text });
    setError(null);
  }

  async function save(event: FormEvent) {
    event.preventDefault();
    if (!draft || busy) return;
    setBusy(true);
    try {
      const [name, next] = await savePlaybook(draft.text);
      setPage(next);
      setDraft(null);
      setNote(`Saved playbook ${name}.`);
    } catch (err) {
      // The editor stays open with the reason.
      setError(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function duplicate(row: PlaybookRow) {
    setBusy(true);
    try {
      const [id, next] = await duplicatePlaybook(row.id);
      setPage(next);
      const copy = next.rows.find((r) => r.id === id);
      if (copy) open(copy);
      setNote(`Made ${copy?.name ?? id}.`);
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function reset(row: PlaybookRow) {
    setBusy(true);
    try {
      setPage(await resetPlaybook(row.id));
      if (draft?.id === row.id) setDraft(null);
      setNote(row.source === "yours" ? `Removed ${row.name}.` : `${row.name} is back to the built-in one.`);
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="playbooks" role="group" aria-label="Playbooks">
      <p className="note">{PLAYBOOKS_NOTE}</p>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      {draft && (
        <form className="card playbook-editor" aria-label={`Edit ${draft.name}`} onSubmit={(e) => void save(e)}>
          <div className="card-title">Edit {draft.name}</div>
          <p className="note">{EDITOR_NOTE}</p>
          <textarea
            aria-label="Playbook TOML"
            className="code"
            rows={18}
            spellCheck={false}
            value={draft.text}
            onChange={(e) => setDraft({ ...draft, text: e.target.value })}
          />
          {error && <p className="note warn">{error}</p>}
          <div className="card-buttons">
            <button type="submit" disabled={busy}>
              Save
            </button>
            <button type="button" className="quiet" onClick={() => setDraft(null)}>
              Cancel
            </button>
          </div>
        </form>
      )}
      {page?.problems.map((problem) => (
        <p key={problem} className="note warn">
          Skipped {problem}
        </p>
      ))}
      {page === null && !note && <p className="empty">Loading playbooks…</p>}
      {page && (
        <ul className="rows" aria-label="All playbooks">
          {page.rows.map((row) => (
            <li key={row.id} className="card" aria-label={row.name}>
              <div className="card-status">{SOURCE_LABEL[row.source]}</div>
              <div className="card-title">{row.name}</div>
              <p className="card-detail-line">{row.summary}</p>
              <p className="card-detail-line">
                {row.fits} · {row.steps}
              </p>
              <div className="card-buttons">
                <button type="button" className="quiet" disabled={busy} onClick={() => open(row)}>
                  {row.source === "shipped" ? "View / Edit" : "Edit"}
                </button>
                <button type="button" className="quiet" disabled={busy} onClick={() => void duplicate(row)}>
                  Duplicate
                </button>
                {row.source !== "shipped" && (
                  <button type="button" className="quiet" disabled={busy} onClick={() => void reset(row)}>
                    {row.source === "yours" ? "Remove" : "Reset"}
                  </button>
                )}
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
