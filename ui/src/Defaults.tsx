import { useState, type FormEvent } from "react";
import { saveDefaults, setModel, type Approvals, type Settings } from "./api";
import { APPROVALS } from "./approvals";
import { PRODUCT_NAME } from "./brand";
import { ModelPicker } from "./ModelPicker";

/** Settings → Defaults: the model new messages use, where the agent works and
 * how chat handles changes. A model pick saves at once, as the composer chip
 * does; the folder and approvals save with the button. `readOnly` is the web
 * app's view. */
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

  if (readOnly) {
    return (
      <dl aria-label="Defaults">
        <dt>Model</dt>
        <dd>{settings.model}</dd>
        <dt>Work folder</dt>
        <dd>
          <code>{settings.workspacePath}</code>
        </dd>
        <dt>Approvals</dt>
        <dd>{APPROVALS[settings.approvals].label}</dd>
      </dl>
    );
  }

  async function pick(id: string) {
    try {
      const next = await setModel(id);
      onSaved?.(next);
      setNote(`New messages use ${next.model}.`);
    } catch (err) {
      setNote(String(err));
    }
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
      <div className="field">
        <span id="defaults-model-label">Model</span>
        <ModelPicker
          value={settings.model}
          onPick={(id) => void pick(id)}
          labelledBy="defaults-model-label"
        />
      </div>
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
        Approvals
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
