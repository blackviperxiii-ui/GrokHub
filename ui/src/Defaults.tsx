import { useState, type FormEvent } from "react";
import { saveDefaults, type Approvals, type Settings } from "./api";
import { APPROVALS, APPROVALS_LABEL } from "./approvals";
import { PRODUCT_NAME } from "./brand";

/** Settings → Defaults: where the agent works and how chat handles changes,
 * saved with the button. There is no model or effort here: the router picks
 * both for every step. `readOnly` is the web app's view. */
export function Defaults({
  settings,
  readOnly = false,
  onSaved,
}: {
  settings: Settings;
  readOnly?: boolean;
  onSaved?: (next: Settings) => void;
}) {
  const [workspace, setWorkspace] = useState(settings.workspace);
  const [approvals, setApprovals] = useState<Approvals>(settings.approvals);
  const [note, setNote] = useState<string | null>(null);
  // Settings stays mounted while hidden and reads the settings again each time
  // it shows. A field edited here and not saved keeps the edit; an untouched
  // one takes the fresh value (Stop asking me on a permission card saves it too).
  const [seen, setSeen] = useState(settings);
  if (settings !== seen) {
    setSeen(settings);
    if (workspace === seen.workspace) setWorkspace(settings.workspace);
    if (approvals === seen.approvals) setApprovals(settings.approvals);
  }

  if (readOnly) {
    return (
      <dl aria-label="Defaults">
        <dt>Work folder</dt>
        <dd>
          <code>{settings.workspacePath}</code>
        </dd>
        <dt>{APPROVALS_LABEL}</dt>
        <dd>{APPROVALS[settings.approvals].label}</dd>
      </dl>
    );
  }

  async function save(event: FormEvent) {
    event.preventDefault();
    try {
      const next = await saveDefaults(workspace.trim(), approvals);
      setWorkspace(next.workspace);
      setApprovals(next.approvals);
      onSaved?.(next);
      setNote("Defaults saved.");
    } catch (err) {
      setNote(String(err));
    }
  }

  return (
    <form onSubmit={(e) => void save(e)}>
      <label>
        Work folder
        <input
          value={workspace}
          placeholder={settings.workspacePath}
          onChange={(e) => setWorkspace(e.target.value)}
        />
      </label>
      <p className="note">
        {PRODUCT_NAME} works in <code>{settings.workspacePath}</code>.
      </p>
      <label>
        {APPROVALS_LABEL}
        <select value={approvals} onChange={(e) => setApprovals(e.target.value as Approvals)}>
          {(Object.keys(APPROVALS) as Approvals[]).map((value) => (
            <option key={value} value={value}>
              {APPROVALS[value].label}
            </option>
          ))}
        </select>
      </label>
      <p className={approvals === "always" ? "note warn" : "note"}>{APPROVALS[approvals].note}</p>
      <button type="submit">Save defaults</button>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
    </form>
  );
}
