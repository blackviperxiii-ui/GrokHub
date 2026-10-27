import { useEffect, useState } from "react";
import { getWebAccess, setWebAccess, type WebAccess as Shape } from "./api";
import { PRODUCT_NAME } from "./brand";

export const PREVIEW_TIP =
  "A preview: it works, but it runs over plain HTTP and has no sign-in beyond the pairing code.";

export const WEB_NOTE =
  `A browser on your network can use ${PRODUCT_NAME} here once it has the pairing code. Chats from the web can't control the desktop, and settings and the API key stay here. A paired browser can approve file changes and commands on this computer, and it's plain HTTP, so use it only on a network you trust.`;
export const WEB_OFF_NOTE =
  `Web access ends when you turn it off or quit ${PRODUCT_NAME}. After 30 minutes with no browser using it, the code changes and browsers pair again.`;
/** How often Settings re-reads the code, so a renewed one shows here. */
export const REFRESH_MS = 30_000;

/** Settings → Web access: serve the web app on this computer's network. */
export function WebAccess() {
  const [web, setWeb] = useState<Shape | null>(null);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    getWebAccess().then(setWeb, (err) => setNote(String(err)));
  }, []);

  useEffect(() => {
    if (!web?.on) return;
    const timer = window.setInterval(() => {
      getWebAccess().then(setWeb, () => {});
    }, REFRESH_MS);
    return () => window.clearInterval(timer);
  }, [web?.on]);

  async function turn(on: boolean) {
    setNote(null);
    try {
      setWeb(await setWebAccess(on));
    } catch (err) {
      setNote(String(err));
    }
  }

  return (
    <div className="web-access" aria-label="Web access" role="group">
      <h2>
        Web access <span className="badge" title={PREVIEW_TIP}>Preview</span>
      </h2>
      <p className="note">{WEB_NOTE}</p>
      {web?.on ? (
        <>
          <p>Open this link in a browser on the same network:</p>
          <ul className="links">
            {web.links.map((link) => (
              <li key={link}>
                <code>{link}</code>
              </li>
            ))}
          </ul>
          <p>
            Or open the address without <code>#code=…</code> and enter the pairing code{" "}
            <code aria-label="Pairing code">{web.code}</code>.
          </p>
          <p className="note">{WEB_OFF_NOTE}</p>
          <button type="button" className="quiet" onClick={() => void turn(false)}>
            Turn off web access
          </button>
        </>
      ) : (
        <button type="button" onClick={() => void turn(true)} disabled={!web}>
          Turn on web access
        </button>
      )}
      {note && <p className="note warn">{note}</p>}
    </div>
  );
}
