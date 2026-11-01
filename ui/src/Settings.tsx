import { useEffect, useEffectEvent, useRef, useState, type FormEvent } from "react";
import {
  clearApiKey,
  getHaltKeys,
  getAutostart,
  getSettings,
  saveSettings,
  setApiKey,
  setAutostart,
  setNotifications,
  setSummonHotkey,
  type Capabilities,
  type Settings as Shape,
} from "./api";
import { inShell } from "./transport";
import { Appearance } from "./Appearance";
import { Background } from "./Background";
import { Behavior } from "./Behavior";
import { Defaults } from "./Defaults";
import { GrokSignIn } from "./GrokSignIn";
import { Labs } from "./Labs";
import { onArrowFocus, usePeek } from "./peek";
import { Interests } from "./Interests";
import { LocalAi } from "./LocalAi";
import { Permissions } from "./Permissions";
import { Personality } from "./Personality";
import { Playbooks } from "./Playbooks";
import { Privacy } from "./Privacy";
import { SECTION_LABELS, sections, type Section } from "./sections";
import { Toggle } from "./Toggle";
import { Updates } from "./Updates";
import { WebAccess } from "./WebAccess";
import { WorkboardTidy } from "./WorkboardTidy";
import { LOGO_MARK_URL, PRODUCT_NAME, TAGLINE } from "./brand";
import { StatusLine, useNote } from "./ui";

export const DESKTOP_NOTE =
  `${PRODUCT_NAME} can see your screen, click and type. Deleting, sending, paying and credentials always wait for your OK.`;

export const GENERAL_EMPTY =
  "Nothing here on this device. The model, work folder and approvals are in Defaults.";

export const HALT_KEYS_NOTE =
  "Halt stops desktop control and every reply: the Halt button, Ctrl+Alt+H in this window, the tray's Halt, or Super+Shift+Esc from any app.";
export const HALT_KEYS_LOCAL =
  "Super+Shift+Esc isn't available here (another app holds it, or this desktop has no system-wide keys). Ctrl+Alt+H, the button and the tray still halt.";
/** The summon hotkey (os_extras::SUMMON_HOTKEY), as this keyboard spells it. */
export const SUMMON_KEYS =
  typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.userAgent)
    ? "⌘+Shift+Space"
    : "Ctrl+Shift+Space";

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
  const [desktopControl, setDesktopControl] = useState(false);
  /** Super+Shift+Esc registered system-wide; null until read. */
  const [globalHalt, setGlobalHalt] = useState<boolean | null>(null);
  /** As the OS has it; null until read. */
  const [autostart, setAutostartShown] = useState<boolean | null>(null);
  const [key, setKey] = useState("");
  const { note, ok, fail, clear } = useNote();
  const [section, setSection] = useState<Section>(goTo?.section ?? "general");
  const nav = useRef<HTMLElement>(null);
  const page = useRef<HTMLDivElement>(null);
  const pane = useRef<HTMLDivElement>(null);
  // On a phone the sections are one row that scrolls; the next one peeks in.
  usePeek(nav, "x", settings !== null && sections(caps).length);

  // A new palette pick opens its section. Settings stays mounted between visits,
  // so showing it again keeps the section that was open.
  const [goToN, setGoToN] = useState(goTo?.n);
  if (goTo && goTo.n !== goToN) {
    setGoToN(goTo.n);
    setSection(goTo.section);
    clear();
  }

  function show(next: Shape) {
    setSettings(next);
    setDesktopControl(next.desktopControl);
  }

  // Runs each time Settings shows. Desktop control changed here and not saved
  // keeps the edit; untouched, it takes the fresh value.
  const refresh = useEffectEvent((next: Shape) => {
    const was = settings;
    if (!was) return show(next);
    setSettings(next);
    if (desktopControl === was.desktopControl) setDesktopControl(next.desktopControl);
  });
  useEffect(() => {
    getSettings().then(
      (next) => refresh(next),
      fail,
    );
  }, [fail]);

  useEffect(() => {
    if (!caps.desktopControl || caps.remote) return;
    getHaltKeys().then(
      (keys) => setGlobalHalt(keys.global),
      () => setGlobalHalt(false),
    );
  }, [caps.desktopControl, caps.remote]);

  useEffect(() => {
    if (!caps.autostart || caps.remote) return;
    getAutostart().then(setAutostartShown, fail);
  }, [caps.autostart, caps.remote, fail]);

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
    if (!settings) return;
    // Defaults and Behavior save the model, tray, folder and approvals themselves.
    const { model, closeToTray, workspace, approvals } = settings;
    try {
      show(await saveSettings(model, closeToTray, workspace, approvals, desktopControl));
      ok("Settings saved.");
    } catch (err) {
      fail(err);
    }
  }

  // These switches apply at once. Only the switch changes, so an unsaved edit
  // in the form above stays.
  async function flipAutostart(on: boolean) {
    try {
      setAutostartShown(await setAutostart(on));
      clear();
    } catch (err) {
      fail(err);
    }
  }

  async function flip(change: () => Promise<Shape>) {
    try {
      setSettings(await change());
      clear();
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
        {section === "defaults" && <Defaults settings={loaded} onSaved={show} />}
        {section === "behavior" && (
          <Behavior
            tray={caps.tray}
            onCloseToTray={(closeToTray) => setSettings({ ...loaded, closeToTray })}
          />
        )}
        {section === "behavior" && <WorkboardTidy />}
        {section === "appearance" && (
          <Appearance theme={loaded.theme} onSaved={(theme) => setSettings({ ...loaded, theme })} />
        )}
        {section === "personality" && <Personality />}
        {section === "playbooks" && <Playbooks />}
        {section === "permissions" && <Permissions />}
        {section === "privacy" && <Privacy />}
        {section === "background" && <Background />}
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
    const onThisComputer = basics();
    if (!caps.desktopControl) return onThisComputer ?? <p className="note">{GENERAL_EMPTY}</p>;
    return (
      <>
        <form onSubmit={save}>
          <Toggle checked={desktopControl} onChange={setDesktopControl}>
            Let {PRODUCT_NAME} control the desktop
          </Toggle>
          {desktopControl && <p className="note warn">{DESKTOP_NOTE}</p>}
          {globalHalt !== null && <p className="note">{globalHalt ? HALT_KEYS_NOTE : HALT_KEYS_LOCAL}</p>}
          <button type="submit">Save settings</button>
        </form>
        {onThisComputer}
      </>
    );
  }

  /** Start at login, the summon hotkey and notifications: desktop only. */
  function basics() {
    if (!caps.autostart && !caps.summonHotkey && !caps.notifications) return null;
    return (
      <div className="settings-group" role="group" aria-label="On this computer">
        <h2>On this computer</h2>
        {caps.autostart && (
          <Toggle
            checked={autostart === true}
            disabled={autostart === null}
            onChange={(on) => void flipAutostart(on)}
          >
            Start {PRODUCT_NAME} when you sign in
          </Toggle>
        )}
        {caps.summonHotkey && (
          <Toggle
            checked={loaded.summonHotkey}
            onChange={(on) => void flip(() => setSummonHotkey(on))}
          >
            Bring up {PRODUCT_NAME} from anywhere with {SUMMON_KEYS}
          </Toggle>
        )}
        {caps.notifications && (
          <Toggle
            checked={loaded.notifications}
            onChange={(on) => void flip(() => setNotifications(on))}
          >
            Notify me when the agent needs an OK or finishes while {PRODUCT_NAME} is in the background
          </Toggle>
        )}
      </div>
    );
  }

  function account() {
    return (
      <>
        {caps.grokSignIn && <GrokSignIn signedIn={loaded.signedIn} onChange={show} />}
        <div className="key">
          <h2>{caps.grokSignIn ? "Or use an xAI API key" : "xAI API key"}</h2>
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
      </>
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
        <dt>xAI API key</dt>
        <dd>{settings.hasApiKey ? "Saved on your computer." : "Not set yet."}</dd>
        <dt>Grok sign-in</dt>
        <dd>{settings.signedIn ? "Signed in on your computer." : "Not signed in."}</dd>
      </dl>
      <h2>{SECTION_LABELS.defaults}</h2>
      <Defaults settings={settings} readOnly />
      <h2>{SECTION_LABELS.behavior}</h2>
      <Behavior tray readOnly />
      <h2>{SECTION_LABELS.privacy}</h2>
      <Privacy readOnly />
      <h2>{SECTION_LABELS.background}</h2>
      <Background />
    </section>
  );
}
