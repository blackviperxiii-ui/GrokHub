import { useEffect, useState } from "react";
import {
  approveCompanion,
  forgetAllDevices,
  getCompanions,
  getDevices,
  getHubJoin,
  getShare,
  joinHub,
  leaveHub,
  newCompanionCode,
  newShareCode,
  removeCompanion,
  setCompanionAllowed,
  startCompanions,
  startShare,
  stopCompanions,
  stopShare,
  type Companion,
  type Companions,
  type Devices as Shape,
  type HubJoin,
  type PairedBrowser,
  type Share,
} from "./api";
import { PageHead, StatusLine, useNote } from "./ui";

/** How often the page re-reads the list, so a browser that just paired shows. */
export const REFRESH_MS = 15_000;

export const DEVICES_NOTE =
  "Browsers paired with Web access right now. The list lives in memory only: it empties when the pairing code changes, web access turns off or the app quits.";
export const OFF_NOTE = "Web access is off, so no browser can use this computer.";
export const NONE_PAIRED = "No browser is paired right now.";
export const SIGNED_OUT =
  "Signed out every browser. To use this computer again, a browser pairs with the new code in Settings → Web access.";

export const SHARE_NOTE =
  "Share lets another computer running this app pair with this one on your network. It is off until you start it and ends when the app quits.";
export const SHARE_OFF = "Not sharing. Start share to pair another computer.";
export const PAIR_STEP = "Open this address on the other computer and enter the code.";
export const PAIRED_NOTE = "Paired. Make a new code after another computer joins.";
export const CODE_GONE = "That code stopped working. Make a new one.";
export const JOIN_NOTE =
  "Join another computer's share to show this one on its Fleet page. Only this computer's CPU, memory and app version are sent, once a minute, on your network.";
export const JOIN_OFF = "Not joined. Type the address and code the other computer shows under Sharing.";

/** "Sending to Desk (192.168.1.20:18766)" and how the last send went. */
export function joinLine(join: HubJoin, now: number): string {
  if (!join.joined) return JOIN_OFF;
  const to = `Sending to ${join.joined.hubName} (${join.joined.address})`;
  if (join.lastError) return `${to}. ${join.lastError}`;
  if (join.lastSentAt !== null) return `${to}. Last sent ${ago(join.lastSentAt, now)}.`;
  return `${to}.`;
}

/** Devices → Join another computer: pair with its share and send this computer's card. */
export function JoinCard() {
  const [join, setJoin] = useState<HubJoin | null>(null);
  const [address, setAddress] = useState("");
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const { note, fail, clear } = useNote();

  useEffect(() => {
    function read() {
      getHubJoin().then((next) => {
        setJoin(next);
        setNow(Date.now());
        clear();
      }, fail);
    }
    read();
    const timer = window.setInterval(read, REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [fail, clear]);

  async function step(run: () => Promise<HubJoin>) {
    setBusy(true);
    try {
      setJoin(await run());
      setNow(Date.now());
      setCode("");
      clear();
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="card device" aria-label="Join">
      <div className="card-title">Joined computer</div>
      <div className="card-status">{join === null ? "Checking…" : joinLine(join, now)}</div>
      {join !== null && !join.joined && (
        <>
          <input
            aria-label="Other computer's address"
            placeholder="http://192.168.1.20:18766"
            value={address}
            onChange={(e) => setAddress(e.target.value)}
          />
          <input
            aria-label="Other computer's code"
            placeholder="Code"
            autoComplete="off"
            value={code}
            onChange={(e) => setCode(e.target.value)}
          />
        </>
      )}
      <StatusLine note={note} />
      <div className="card-buttons">
        {join?.joined ? (
          <button type="button" className="quiet" disabled={busy} onClick={() => void step(leaveHub)}>
            Leave
          </button>
        ) : (
          <button
            type="button"
            disabled={busy || join === null || !address.trim() || !code.trim()}
            onClick={() => void step(() => joinHub(address, code))}
          >
            {busy ? "Joining…" : "Join"}
          </button>
        )}
      </div>
    </div>
  );
}

export const COMPANIONS_NOTE =
  "A companion is a phone or another computer that lends this app what it has: its camera, screen, location, text messages, voice, a panel to show things on, and its own MCP servers and skills. It lends only what you allow here, and each use asks first like any other tool.";
export const COMPANIONS_OFF = "Off. Start companions to pair a phone or another computer.";
export const COMPANION_STEP = "On the companion, run:";
export const NO_OFFERS = "It offers nothing yet. On the companion, run grokhub-hub node offer camera,location (or what it should lend).";
export const COMPANION_REFRESH_MS = 5_000;

/** What each offer is called on the page. */
export const OFFER_LABELS: Record<string, string> = {
  camera: "Camera",
  screen: "Screen",
  location: "Location",
  sms: "Text messages",
  talk: "Voice",
  wake: "Wake",
  canvas: "Panels",
  mcp: "Its MCP servers",
  skills: "Its skills",
};

const PLATFORMS: Record<string, string> = {
  android: "Android",
  linux: "Linux",
  macos: "macOS",
  windows: "Windows",
};

/** What the person types on the companion to pair it. */
export function pairCommand(view: Companions): string {
  return `grokhub-hub node pair ${view.address || "<this computer's address>"} ${view.code}`;
}

/** "Android · Online · Paired 5 min ago". */
export function companionLine(c: Companion, now: number): string {
  const state = !c.approved ? "Waiting for your yes" : c.online ? "Online" : "Offline";
  return `${PLATFORMS[c.platform] ?? "Other device"} · ${state} · Paired ${ago(c.pairedAt, now)}`;
}

function sameSet(a: string[], b: string[]): boolean {
  return a.length === b.length && a.every((x) => b.includes(x));
}

/** One companion: what it offers, what you allow, and Remove. */
export function CompanionRow({
  companion,
  now,
  busy,
  onSave,
  onRemove,
}: {
  companion: Companion;
  now: number;
  busy: boolean;
  onSave: (allowed: string[]) => void;
  onRemove: () => void;
}) {
  const saved = companion.approved ? companion.allowed : [];
  const savedKey = saved.join(",");
  const [picked, setPicked] = useState<string[]>(saved);
  // A save, or a change made elsewhere, resets the boxes to what is allowed now.
  const [shown, setShown] = useState(savedKey);
  if (shown !== savedKey) {
    setShown(savedKey);
    setPicked(saved);
  }

  function toggle(offer: string) {
    setPicked((now) => (now.includes(offer) ? now.filter((o) => o !== offer) : [...now, offer]));
  }

  return (
    <li className="card device" aria-label={companion.name}>
      <div className="card-title">{companion.name}</div>
      <div className="card-status">{companionLine(companion, now)}</div>
      {companion.offers.length === 0 ? (
        <div className="card-detail-line">{NO_OFFERS}</div>
      ) : (
        <fieldset className="companion-offers">
          <legend>It may lend</legend>
          {companion.offers.map((offer) => (
            <label key={offer}>
              <input
                type="checkbox"
                checked={picked.includes(offer)}
                disabled={busy}
                onChange={() => toggle(offer)}
              />
              {OFFER_LABELS[offer] ?? offer}
            </label>
          ))}
        </fieldset>
      )}
      <div className="card-buttons">
        {companion.approved ? (
          <button type="button" disabled={busy || sameSet(picked, saved)} onClick={() => onSave(picked)}>
            Save
          </button>
        ) : (
          <button type="button" disabled={busy} onClick={() => onSave(picked)}>
            Allow
          </button>
        )}
        <button type="button" className="danger" disabled={busy} onClick={onRemove}>
          Remove
        </button>
      </div>
    </li>
  );
}

/** Devices → Companions: start the host, pair with a code, allow and remove. */
export function CompanionsCard() {
  const [view, setView] = useState<Companions | null>(null);
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(() => Date.now());
  const { note, fail, clear } = useNote();

  useEffect(() => {
    function read() {
      getCompanions().then((next) => {
        setView(next);
        setNow(Date.now());
      }, fail);
    }
    read();
    const timer = window.setInterval(read, COMPANION_REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [fail]);

  async function step(run: () => Promise<Companions>) {
    setBusy(true);
    try {
      setView(await run());
      setNow(Date.now());
      clear();
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  }

  return (
    <>
      <div className="card device" aria-label="Companions">
        <div className="card-title">Companions</div>
        <div className="card-status">
          {view === null ? "Checking…" : view.on ? `On, port ${view.port}` : COMPANIONS_OFF}
        </div>
        {view?.on && view.code && (
          <>
            <p className="device-code" aria-label="Companion pairing code">
              {view.code}
            </p>
            <div className="card-detail-line">{COMPANION_STEP}</div>
            <code className="card-detail-line">{pairCommand(view)}</code>
          </>
        )}
        <StatusLine note={note} />
        <div className="card-buttons">
          {view?.on ? (
            <>
              <button type="button" className="quiet" disabled={busy} onClick={() => void step(newCompanionCode)}>
                New code
              </button>
              <button type="button" className="quiet" disabled={busy} onClick={() => void step(stopCompanions)}>
                Stop companions
              </button>
            </>
          ) : (
            <button type="button" disabled={busy || view === null} onClick={() => void step(startCompanions)}>
              Start companions
            </button>
          )}
        </div>
      </div>
      {view && view.companions.length > 0 && (
        <ul className="rows" aria-label="Paired companions">
          {view.companions.map((c) => (
            <CompanionRow
              key={c.id}
              companion={c}
              now={now}
              busy={busy}
              onSave={(allowed) =>
                void step(() => (c.approved ? setCompanionAllowed(c.id, allowed) : approveCompanion(c.id, allowed)))
              }
              onRemove={() => void step(() => removeCompanion(c.id))}
            />
          ))}
        </ul>
      )}
    </>
  );
}

export const RUN_NOTE =
  "Opens a chat with this task, so it runs on this PC with the same approvals as anything else you ask.";

/** "just now", "5 min ago", "2 h ago", "3 days ago". */
export function ago(at: number, now: number): string {
  const minutes = Math.floor(Math.max(0, now - at) / 60_000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.floor(hours / 24);
  return `${days} ${days === 1 ? "day" : "days"} ago`;
}

export function browserLine(browser: PairedBrowser, now: number): string {
  const chats = `${browser.chats} ${browser.chats === 1 ? "chat" : "chats"}`;
  return `Paired ${ago(browser.firstSeen, now)} · Last used ${ago(browser.lastCall, now)} · ${chats}`;
}

/** This computer, Web access, and the browsers paired with it right now. */
export function Devices({
  onOpenWebAccess,
  onRun,
}: {
  onOpenWebAccess: () => void;
  onRun?: (text: string) => void;
}) {
  const [share, setShare] = useState<Share | null>(null);
  const [task, setTask] = useState("");
  const [devices, setDevices] = useState<Shape | null>(null);
  const { note, ok, fail } = useNote();
  const [busy, setBusy] = useState(false);
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    function read() {
      getDevices().then(
        (next) => {
          setDevices(next);
          setNow(Date.now());
        },
        fail,
      );
    }
    read();
    const timer = window.setInterval(read, REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [fail]);

  useEffect(() => {
    function read() {
      getShare().then(setShare, fail);
    }
    read();
    const timer = window.setInterval(read, REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [fail]);

  async function shareStep(step: () => Promise<Share>) {
    setBusy(true);
    try {
      setShare(await step());
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  }

  function runHere() {
    const text = task.trim();
    if (!text || !onRun) return;
    setTask("");
    onRun(text);
  }

  async function signOutAll() {
    setBusy(true);
    try {
      setDevices(await forgetAllDevices());
      setNow(Date.now());
      ok(SIGNED_OUT);
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="devices" aria-label="Devices">
      <PageHead title="Devices" />
      <div className="card device" aria-label="This computer">
        <div className="card-title">{devices?.computer ?? "This computer"}</div>
        <div className="card-status">This computer</div>
        {devices?.on && devices.lan && <div className="card-detail-line">{devices.lan}</div>}
      </div>
      <div className="card device" aria-label="Web access">
        <div className="card-title">Web access</div>
        <div className="card-status">{devices === null ? "Checking…" : devices.on ? "On" : "Off"}</div>
        <div className="card-buttons">
          <button type="button" className="quiet" onClick={onOpenWebAccess}>
            Open Web access settings
          </button>
        </div>
      </div>
      <h2>Share with another computer</h2>
      <p className="note">{SHARE_NOTE}</p>
      <div className="card device" aria-label="Share">
        <div className="card-title">Sharing</div>
        <div className="card-status">
          {share === null ? "Checking…" : share.on ? `On, port ${share.port}` : SHARE_OFF}
        </div>
        {share?.on && share.code && (
          <>
            <p className="device-code" aria-label="Pairing code">
              {share.code}
            </p>
            <div className="card-detail-line">{`${PAIR_STEP} ${share.address}`}</div>
          </>
        )}
        {share?.on && !share.code && (
          <div className="card-detail-line">{share.pairedSinceCode ? PAIRED_NOTE : CODE_GONE}</div>
        )}
        <div className="card-buttons">
          {share?.on ? (
            <>
              <button type="button" className="quiet" disabled={busy} onClick={() => void shareStep(newShareCode)}>
                New code
              </button>
              <button type="button" className="quiet" disabled={busy} onClick={() => void shareStep(stopShare)}>
                Stop share
              </button>
            </>
          ) : (
            <button type="button" disabled={busy || share === null} onClick={() => void shareStep(startShare)}>
              Start share
            </button>
          )}
        </div>
      </div>
      {share && share.peers.length > 0 && (
        <ul className="rows" aria-label="Paired computers">
          {share.peers.map((peer) => (
            <li key={peer.id} className="card device">
              <div className="card-title">{peer.name}</div>
              <div className="card-status">
                {peer.lastSeen > 0 ? `Last used ${ago(peer.lastSeen, now)}` : "Paired, not used yet"}
              </div>
            </li>
          ))}
        </ul>
      )}
      <h2>Join another computer</h2>
      <p className="note">{JOIN_NOTE}</p>
      <JoinCard />
      <h2>Companions</h2>
      <p className="note">{COMPANIONS_NOTE}</p>
      <CompanionsCard />
      <h2>Run a task on this PC</h2>
      <p className="note">{RUN_NOTE}</p>
      <textarea
        aria-label="Task for this PC"
        rows={3}
        placeholder="What should this computer do?"
        value={task}
        onChange={(e) => setTask(e.target.value)}
      />
      <div className="card-buttons">
        <button type="button" disabled={!task.trim() || !onRun} onClick={runHere}>
          Run it here
        </button>
      </div>
      <h2>Paired browsers</h2>
      <p className="note">{DEVICES_NOTE}</p>
      <StatusLine note={note} />
      {devices && !devices.on && <p className="empty">{OFF_NOTE}</p>}
      {devices?.on && devices.browsers.length === 0 && <p className="empty">{NONE_PAIRED}</p>}
      {devices?.on && devices.browsers.length > 0 && (
        <ul className="rows" aria-label="Paired browsers">
          {devices.browsers.map((browser, i) => (
            <li key={`${browser.name}-${browser.firstSeen}-${i}`} className="card device">
              <div className="card-title">{browser.name}</div>
              <div className="card-status">{browserLine(browser, now)}</div>
            </li>
          ))}
        </ul>
      )}
      <div className="card-buttons">
        <button
          type="button"
          className="danger"
          disabled={busy || !devices?.on}
          onClick={() => void signOutAll()}
        >
          {busy ? "Signing out…" : "Sign out all browsers"}
        </button>
      </div>
    </section>
  );
}
