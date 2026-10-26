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
import { APPROVALS } from "./approvals";
import { Labs } from "./Labs";
import { Interests } from "./Interests";
import { LocalAi } from "./LocalAi";
import { Personality } from "./Personality";
import { Playbooks } from "./Playbooks";
import { SECTION_LABELS, sections, type Section } from "./sections";
import { WebAccess } from "./WebAccess";

export const DESKTOP_NOTE =
  "Grok can see your screen, click and type. Deleting, sending, paying and credentials always wait for your OK.";

/** `goTo`: a section the command palette picked. A new `n` opens it. */
export function Settings({
  caps,
  goTo,
}: {
  caps: Capabilities;
  goTo?: { section: Section; n: number };
}) {
  const [settings, setSettings] = useState<Shape | null>(null);
  const [model, setModel] = useState("");
  const [closeToTray, setCloseToTray] = useState(true);
  const [workspace, setWorkspace] = useState("");
  const [approvals, setApprovals] = useState<Approvals>("ask");
  const [desktopControl, setDesktopControl] = useState(false);
  const [key, setKey] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [section, setSection] = useState<Section>(goTo?.section ?? "general");

  useEffect(() => {
    if (goTo) {
      setSection(goTo.section);
      setNote(null);
    }
  }, [goTo]);

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

  const loaded = settings;

  function open(next: Section) {
    setSection(next);
    setNote(null);
  }

  return (
    <div className="settings-page">
      <nav className="settings-nav" aria-label="Settings sections">
        {sections(caps).map((name) => (
          <button
            key={name}
            type="button"
            aria-pressed={section === name}
            onClick={() => open(name)}
          >
            {SECTION_LABELS[name]}
          </button>
        ))}
      </nav>
      <section className="settings" aria-label={SECTION_LABELS[section]}>
        {section === "general" && general()}
        {section === "account" && account()}
        {note && <p className="note">{note}</p>}
        {section === "personality" && <Personality />}
        {section === "playbooks" && <Playbooks />}
        {section === "pulse" && caps.pulse && <Interests />}
        {section === "localAi" && caps.localAi && <LocalAi />}
        {section === "webAccess" && caps.webAccess && <WebAccess />}
        {section === "labs" && caps.plugins && <Labs />}
        {section === "about" && <About version={loaded.version} />}
      </section>
    </div>
  );

  function general() {
    return (
      <form onSubmit={save}>
        <label>
          Model
          <input value={model} onChange={(e) => setModel(e.target.value)} />
        </label>
        <label>
          Work folder
          <input
            value={workspace}
            placeholder={loaded.workspacePath}
            onChange={(e) => setWorkspace(e.target.value)}
          />
        </label>
        <p className="note">
          GrokHub works in <code>{loaded.workspacePath}</code>.
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
    );
  }

  function account() {
    return (
      <div className="key">
        <h2>xAI API key</h2>
        <p>{loaded.hasApiKey ? "A key is saved." : "No key saved yet."}</p>
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
        {loaded.hasApiKey && (
          <button type="button" className="quiet" onClick={removeKey}>
            Remove key
          </button>
        )}
      </div>
    );
  }
}

export const ABOUT_NOTE =
  "GrokHub Shell is the cross-platform app. The GrokHub cabin keeps working next to it until the shell has everything the cabin has.";

function About({ version }: { version: string }) {
  return (
    <div className="about">
      <h2>GrokHub Shell</h2>
      <p>Version {version}</p>
      <p className="note">{ABOUT_NOTE}</p>
    </div>
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
