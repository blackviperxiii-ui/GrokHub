import { useEffect, useState } from "react";
import { forgetAllDevices, getDevices, type Devices as Shape, type PairedBrowser } from "./api";
import { StatusLine, useNote } from "./ui";

/** How often the page re-reads the list, so a browser that just paired shows. */
export const REFRESH_MS = 15_000;

export const DEVICES_NOTE =
  "Browsers paired with Web access right now. The list lives in memory only: it empties when the pairing code changes, web access turns off or the app quits.";
export const OFF_NOTE = "Web access is off, so no browser can use this computer.";
export const NONE_PAIRED = "No browser is paired right now.";
export const SIGNED_OUT =
  "Signed out every browser. To use this computer again, a browser pairs with the new code in Settings → Web access.";

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
export function Devices({ onOpenWebAccess }: { onOpenWebAccess: () => void }) {
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
  }, []);

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
