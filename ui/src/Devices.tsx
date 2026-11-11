import { useEffect, useState } from "react";
import {
  forgetAllDevices,
  getDevices,
  getShare,
  newShareCode,
  startShare,
  stopShare,
  type Devices as Shape,
  type PairedBrowser,
  type Share,
} from "./api";
import { StatusLine, useNote } from "./ui";

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
        {share?.on && !share.code && <div className="card-detail-line">{PAIRED_NOTE}</div>}
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
            <li key={peer.name} className="card device">
              <div className="card-title">{peer.name}</div>
              <div className="card-status">
                {peer.lastSeen > 0 ? `Last used ${ago(peer.lastSeen, now)}` : "Paired, not used yet"}
              </div>
            </li>
          ))}
        </ul>
      )}
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
