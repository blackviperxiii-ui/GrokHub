import { useEffect, useState } from "react";
import { getPrivacy, openDataFolder, setOfflineOnly, type PrivacyPage } from "./api";
import { PRODUCT_NAME } from "./brand";
import { Toggle } from "./Toggle";

export const OFFLINE_NOTE = `${PRODUCT_NAME} stays off the internet. Chat uses Local AI if it's set up; web search, cloud models and online connectors pause.`;
export const OFFLINE_NOT_READY =
  "No local model is ready, and nothing can be downloaded while Offline only is on.";

/** Settings → Privacy & data: Offline only (the same offline.toml the cabin
 * reads), what left this computer this week, the data folder and whether
 * memory is sealed. It reads again whenever the window gets focus, so a flip
 * in the cabin shows here. `readOnly` is the web app's view. */
export function Privacy({ readOnly = false }: { readOnly?: boolean }) {
  const [page, setPage] = useState<PrivacyPage | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    const load = () => getPrivacy().then(setPage, (err) => setNote(String(err)));
    void load();
    window.addEventListener("focus", load);
    return () => window.removeEventListener("focus", load);
  }, []);

  if (!page) return <p className="note">{note ?? "Loading…"}</p>;

  async function offline(on: boolean) {
    setConfirming(false);
    try {
      setPage(await setOfflineOnly(on));
      setNote(on ? "Offline only is on." : "Offline only is off.");
    } catch (err) {
      setNote(String(err));
    }
  }

  function flip(on: boolean) {
    if (on && !page?.localReady) {
      setConfirming(true);
      return;
    }
    void offline(on);
  }

  async function openFolder() {
    try {
      await openDataFolder();
    } catch (err) {
      setNote(String(err));
    }
  }

  const week = (
    <>
      <h2>Off-device this week</h2>
      <p>{page.sent}</p>
      {page.hosts.length > 0 && (
        <ul className="privacy-hosts" aria-label="Hosts this week">
          {page.hosts.map((h) => (
            <li key={h.host}>
              <code>{h.host}</code> · {h.calls === 1 ? "1 call" : `${h.calls} calls`}
            </li>
          ))}
        </ul>
      )}
    </>
  );

  if (readOnly) {
    return (
      <div className="privacy" aria-label="Privacy & data">
        <dl>
          <dt>Offline only</dt>
          <dd>{page.offlineOnly ? "On" : "Off"}</dd>
          <dt>Encrypted memory</dt>
          <dd>{page.encryptedNote}</dd>
        </dl>
        {week}
      </div>
    );
  }

  return (
    <div className="privacy" role="group" aria-label="Privacy & data">
      <Toggle checked={confirming || page.offlineOnly} onChange={flip}>
        Offline only
      </Toggle>
      <p className="note">{OFFLINE_NOTE}</p>
      {confirming && (
        <div className="confirm">
          <p className="note warn">{OFFLINE_NOT_READY}</p>
          <button type="button" onClick={() => void offline(true)}>
            Turn on anyway
          </button>
          <button type="button" className="quiet" onClick={() => setConfirming(false)}>
            Cancel
          </button>
        </div>
      )}
      <Toggle checked={page.encrypted} disabled onChange={() => {}}>
        Encrypted memory
      </Toggle>
      <p className="note">{page.encryptedNote}</p>
      <p className="note">
        Your data lives in <code>{page.dataFolder}</code>.
      </p>
      {page.canOpenFolder && (
        <button type="button" className="quiet" onClick={() => void openFolder()}>
          Open data folder
        </button>
      )}
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      {week}
    </div>
  );
}
