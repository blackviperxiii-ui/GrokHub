import { useEffect, useRef, useState, type FormEvent } from "react";
import {
  clearApiKey,
  getHaltKeys,
  getSettings,
  saveSettings,
  setApiKey,
  type Approvals,
  type Capabilities,
  type Settings as Shape,
} from "./api";
import { inShell } from "./transport";
import { Appearance } from "./Appearance";
import { APPROVALS } from "./approvals";
import { Labs } from "./Labs";
import { ModelPicker } from "./ModelPicker";
import { onArrowFocus, usePeek } from "./peek";
import { Interests } from "./Interests";
import { LocalAi } from "./LocalAi";
import { Permissions } from "./Permissions";
import { Personality } from "./Personality";
import { Playbooks } from "./Playbooks";
import { SECTION_LABELS, sections, type Section } from "./sections";
import { Toggle } from "./Toggle";
import { Updates } from "./Updates";
import { WebAccess } from "./WebAccess";
import { LOGO_MARK_URL, PRODUCT_NAME, TAGLINE } from "./brand";
import { StatusLine, useNote } from "./ui";

export const DESKTOP_NOTE =
  "Grok can see your screen, click and type. Deleting, sending, paying and credentials always wait for your OK.";

export const HALT_KEYS_NOTE =
  "Halt stops desktop control and every reply: the Halt button, Ctrl+Alt+H in this window, the tray's Halt, or Super+Shift+Esc from any app.";
export const HALT_KEYS_LOCAL =
  "Super+Shift+Esc isn't available here (another app holds it, or this desktop has no system-wide keys). Ctrl+Alt+H, the button and the tray still halt.";

/** `goTo`: a section the command palette picked. A new `n` opens it. */
export function Settings({
  caps,
  goTo,
  onClose,
}: {
  caps: Capabilities;
  goTo?: { section: Section; n: number };
  /** Leaves Settings for the screen that was open before it. */
  onClose?: () => void;
}) {
  const [settings, setSettings] = useState<Shape | null>(null);
  const [model, setModel] = useState("");
  const [closeToTray, setCloseToTray] = useState(true);
  const [workspace, setWorkspace] = useState("");
  const [approvals, setApprovals] = useState<Approvals>("ask");
  const [desktopControl, setDesktopControl] = useState(false);
  /** Super+Shift+Esc registered system-wide; null until read. */
  const [globalHalt, setGlobalHalt] = useState<boolean | null>(null);
  const [key, setKey] = useState("");
  const { note, ok, fail, clear } = useNote();
  const [section, setSection] = useState<Section>(goTo?.section ?? "general");
  const nav = useRef<HTMLElement>(null);
  const page = useRef<HTMLDivElement>(null);
  const pane = useRef<HTMLDivElement>(null);
  // On a phone the sections are one row that scrolls; the next one peeks in.
  usePeek(nav, "x", settings !== null && sections(caps).length);

  useEffect(() => {
    if (goTo) {
      setSection(goTo.section);
      clear();
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
    getSettings().then(show, fail);
  }, []);

  useEffect(() => {
    if (!caps.desktopControl || caps.remote) return;
    getHaltKeys().then(
      (keys) => setGlobalHalt(keys.global),
      () => setGlobalHalt(false),
    );
  }, [caps.desktopControl, caps.remote]);

  // Each section is its own page: it opens at the top, however far the last one was read.
  useEffect(() => {
    if (pane.current) pane.current.scrollTop = 0;
    if (page.current) page.current.scrollTop = 0;
  }, [section, goTo]);

  useEffect(() => {
    if (!onClose) return;
    function onKey(event: KeyboardEvent) {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      // Esc belongs to a field or list that is using it, such as the palette.
      const target = event.target as HTMLElement | null;
      if (target?.closest("[role='dialog'], [role='listbox'], select")) return;
      onClose?.();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  async function save(event: FormEvent) {
    event.preventDefault();
    try {
      show(
        await saveSettings(model.trim(), closeToTray, workspace.trim(), approvals, desktopControl),
      );
      ok("Settings saved.");
    } catch (err) {
      fail(err);
    }
  }

  async function saveKey() {
    if (!key.trim()) return;
    try {
      show(await setApiKey(key.trim()));
      setKey("");
      ok("API key saved in your system keychain.");
    } catch (err) {
      fail(err);
    }
  }

  async function removeKey() {
    try {
      show(await clearApiKey());
      ok("API key removed.");
    } catch (err) {
      fail(err);
    }
  }

  if (!settings) {
    return <section className="settings">{note?.text ?? "Loading settings…"}</section>;
  }

  if (caps.remote) return <RemoteSettings settings={settings} />;

  const loaded = settings;

  function open(next: Section) {
    setSection(next);
    clear();
  }

  return (
    <div className="settings-page" ref={page}>
      <nav
        ref={nav}
        className="settings-nav"
        aria-label="Settings sections"
        onKeyDown={(e) => onArrowFocus(e)}
      >
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
      <div className="settings-pane" ref={pane}>
        <header className="settings-head">
          <h1>{SECTION_LABELS[section]}</h1>
          {onClose && (
            <button type="button" className="quiet" onClick={onClose} aria-label="Close settings">
              Close
            </button>
          )}
        </header>
        <section className="settings" aria-label={SECTION_LABELS[section]}>
        {section === "general" && general()}
        {section === "account" && account()}
        <StatusLine note={note} className="note" />
        {section === "appearance" && (
          <Appearance theme={loaded.theme} onSaved={(theme) => setSettings({ ...loaded, theme })} />
        )}
        {section === "personality" && <Personality />}
        {section === "playbooks" && <Playbooks />}
        {section === "permissions" && <Permissions />}
        {section === "pulse" && caps.pulse && <Interests />}
        {section === "localAi" && caps.localAi && <LocalAi />}
        {section === "webAccess" && caps.webAccess && <WebAccess />}
        {section === "labs" && caps.plugins && <Labs />}
        {section === "about" && <About version={loaded.version} />}
        </section>
      </div>
    </div>
  );

  function general() {
    return (
      <form onSubmit={save}>
        <div className="field">
          <span id="model-label">Model</span>
          <ModelPicker value={model} onPick={setModel} labelledBy="model-label" />
        </div>
        <label>
          Work folder
          <input
            value={workspace}
            placeholder={loaded.workspacePath}
            onChange={(e) => setWorkspace(e.target.value)}
          />
        </label>
        <p className="note">
          {PRODUCT_NAME} works in <code>{loaded.workspacePath}</code>.
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
          <Toggle checked={desktopControl} onChange={setDesktopControl}>
            Let Grok control the desktop
          </Toggle>
        )}
        {caps.desktopControl && desktopControl && <p className="note warn">{DESKTOP_NOTE}</p>}
        {caps.desktopControl && globalHalt !== null && (
          <p className="note">{globalHalt ? HALT_KEYS_NOTE : HALT_KEYS_LOCAL}</p>
        )}
        {caps.tray && (
          <Toggle checked={closeToTray} onChange={setCloseToTray}>
            Closing the window keeps {PRODUCT_NAME} in the tray
          </Toggle>
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
  `This is the cross-platform app. The ${PRODUCT_NAME} cabin keeps working next to it until this app has everything the cabin has.`;

function About({ version }: { version: string }) {
  return (
    <div className="about">
      <h2 className="brand-lockup">
        <img src={LOGO_MARK_URL} alt="" />
        {PRODUCT_NAME}
      </h2>
      <p className="note">{TAGLINE}</p>
      <p>Version {version}</p>
      <p className="note">{ABOUT_NOTE}</p>
      {inShell() && <Updates />}
    </div>
  );
}

export const REMOTE_NOTE =
  `You're using ${PRODUCT_NAME} on your computer from this browser. Change settings and the API key there.`;

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
