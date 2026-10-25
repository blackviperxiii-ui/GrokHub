import { useEffect, useState, type FormEvent } from "react";
import {
  clearApiKey,
  getSettings,
  saveSettings,
  setApiKey,
  type Approvals,
  type Capabilities,
  type Settings as Shape,
} from "./api";
import { LocalAi } from "./LocalAi";
import { WebAccess } from "./WebAccess";

const APPROVALS: Record<Approvals, { label: string; note: string }> = {
  ask: {
    label: "Ask every time",
    note: "It asks before it changes a file or runs a command.",
  },
  auto: {
    label: "Auto",
    note: "A quick check approves routine steps and asks you about the rest.",
  },
  always: {
    label: "Always approve",
    note: "Files change and commands run without asking. Deleting, sending, paying and credentials still ask.",
  },
};

export const DESKTOP_NOTE =
  "Grok can see your screen, click and type. Deleting, sending, paying and credentials always wait for your OK.";

export function Settings({ caps }: { caps: Capabilities }) {
  const [settings, setSettings] = useState<Shape | null>(null);
  const [model, setModel] = useState("");
  const [closeToTray, setCloseToTray] = useState(true);
  const [workspace, setWorkspace] = useState("");
  const [approvals, setApprovals] = useState<Approvals>("ask");
  const [desktopControl, setDesktopControl] = useState(false);
  const [key, setKey] = useState("");
  const [note, setNote] = useState<string | null>(null);

  function show(next: Shape) {
    setSettings(next);
    setModel(next.model);
    setCloseToTray(next.closeToTray);
    setWorkspace(next.workspace);
    setApprovals(next.approvals);
    setDesktopControl(next.desktopControl);
  }

  useEffect(() => {
    getSettings().then(show, (err) => setNote(String(err)));
  }, []);

  async function save(event: FormEvent) {
    event.preventDefault();
    try {
      show(
        await saveSettings(model.trim(), closeToTray, workspace.trim(), approvals, desktopControl),
      );
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

  if (caps.remote) return <RemoteSettings settings={settings} />;

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
          GrokHub works in <code>{settings.workspacePath}</code>.
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
        <p className={approvals === "always" ? "note warn" : "note"}>
          {APPROVALS[approvals].note}
        </p>
        {caps.desktopControl && (
          <label className="check">
            <input
              type="checkbox"
              checked={desktopControl}
              onChange={(e) => setDesktopControl(e.target.checked)}
            />
            Let Grok control the desktop
          </label>
        )}
        {caps.desktopControl && desktopControl && <p className="note warn">{DESKTOP_NOTE}</p>}
        {caps.tray && (
          <label className="check">
            <input
              type="checkbox"
              checked={closeToTray}
              onChange={(e) => setCloseToTray(e.target.checked)}
            />
            Closing the window keeps GrokHub in the tray
          </label>
        )}
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
      {caps.webAccess && <WebAccess />}
      {caps.localAi && <LocalAi />}
    </section>
  );
}

export const REMOTE_NOTE =
  "You're using GrokHub on your computer from this browser. Change settings and the API key there.";

/** The web app shows the desktop's settings but can't change them. */
function RemoteSettings({ settings }: { settings: Shape }) {
  return (
    <section className="settings" aria-label="Settings">
      <p className="note">{REMOTE_NOTE}</p>
      <dl>
        <dt>Model</dt>
        <dd>{settings.model}</dd>
        <dt>Work folder</dt>
        <dd>
          <code>{settings.workspacePath}</code>
        </dd>
        <dt>Approvals</dt>
        <dd>{APPROVALS[settings.approvals].label}</dd>
        <dt>xAI API key</dt>
        <dd>{settings.hasApiKey ? "Saved on your computer." : "Not set yet."}</dd>
      </dl>
    </section>
  );
}
