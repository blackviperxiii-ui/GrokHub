import { useEffect, useState, type FormEvent } from "react";
import { clearApiKey, getSettings, saveSettings, setApiKey, type Settings as Shape } from "./api";

export function Settings() {
  const [settings, setSettings] = useState<Shape | null>(null);
  const [model, setModel] = useState("");
  const [closeToTray, setCloseToTray] = useState(true);
  const [workspace, setWorkspace] = useState("");
  const [key, setKey] = useState("");
  const [note, setNote] = useState<string | null>(null);

  function show(next: Shape) {
    setSettings(next);
    setModel(next.model);
    setCloseToTray(next.closeToTray);
    setWorkspace(next.workspace);
  }

  useEffect(() => {
    getSettings().then(show, (err) => setNote(String(err)));
  }, []);

  async function save(event: FormEvent) {
    event.preventDefault();
    try {
      show(await saveSettings(model.trim(), closeToTray, workspace.trim()));
      setNote("Settings saved.");
    } catch (err) {
      setNote(String(err));
    }
  }

  async function saveKey() {
    if (!key.trim()) return;
    try {
      show(await setApiKey(key.trim()));
      setKey("");
      setNote("API key saved in your system keychain.");
    } catch (err) {
      setNote(String(err));
    }
  }

  async function removeKey() {
    try {
      show(await clearApiKey());
      setNote("API key removed.");
    } catch (err) {
      setNote(String(err));
    }
  }

  if (!settings) {
    return <section className="settings">{note ?? "Loading settings…"}</section>;
  }

  return (
    <section className="settings" aria-label="Settings">
      <form onSubmit={save}>
        <label>
          Model
          <input value={model} onChange={(e) => setModel(e.target.value)} />
        </label>
        <label>
          Work folder
          <input
            value={workspace}
            placeholder={settings.workspacePath}
            onChange={(e) => setWorkspace(e.target.value)}
          />
        </label>
        <p className="note">
          GrokHub works in <code>{settings.workspacePath}</code>. It asks before it changes a
          file or runs a command.
        </p>
        <label className="check">
          <input
            type="checkbox"
            checked={closeToTray}
            onChange={(e) => setCloseToTray(e.target.checked)}
          />
          Closing the window keeps GrokHub in the tray
        </label>
        <button type="submit">Save settings</button>
      </form>
      <div className="key">
        <h2>xAI API key</h2>
        <p>{settings.hasApiKey ? "A key is saved." : "No key saved yet."}</p>
        <input
          type="password"
          aria-label="API key"
          placeholder="xai-…"
          autoComplete="off"
          value={key}
          onChange={(e) => setKey(e.target.value)}
        />
        <button type="button" onClick={saveKey} disabled={!key.trim()}>
          Save key
        </button>
        {settings.hasApiKey && (
          <button type="button" className="quiet" onClick={removeKey}>
            Remove key
          </button>
        )}
      </div>
      {note && <p className="note">{note}</p>}
    </section>
  );
}
