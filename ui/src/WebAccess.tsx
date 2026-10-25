import { useEffect, useState } from "react";
import { getWebAccess, setWebAccess, type WebAccess as Shape } from "./api";

export const WEB_NOTE =
  "A browser on your network can use GrokHub here once it has the pairing code. Chats from the web can't control the desktop, and settings and the API key stay here. It's plain HTTP, so use it only on a network you trust.";
export const WEB_OFF_NOTE = "Web access ends when you turn it off or quit GrokHub.";

/** Settings → Web access: serve the web app on this computer's network. */
export function WebAccess() {
  const [web, setWeb] = useState<Shape | null>(null);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    getWebAccess().then(setWeb, (err) => setNote(String(err)));
  }, []);

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
      <h2>Web access</h2>
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
