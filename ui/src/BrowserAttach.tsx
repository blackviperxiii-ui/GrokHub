import { useEffect, useState, type FormEvent } from "react";
import { getBrowserAttach, saveBrowserAttach, type BrowserAttach as Shape } from "./browserAttachApi";
import { PRODUCT_NAME } from "./brand";

export const ATTACH_NOTE = `${PRODUCT_NAME}'s browser tool can work in your own browser, where you are signed in, but only on the sites listed here. Start Chrome or Edge with --remote-debugging-port=9222 first. With no sites listed it keeps its own browser.`;

/** Settings → General → Your browser. Hidden in the web app. */
export function BrowserAttach() {
  const [shown, setShown] = useState<Shape | null>(null);
  const [endpoint, setEndpoint] = useState("");
  const [site, setSite] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    getBrowserAttach().then(
      (next) => {
        setShown(next);
        if (next) setEndpoint(next.endpoint);
      },
      () => setShown(null),
    );
  }, []);

  if (!shown) return null;

  async function save(origins: string[], at = endpoint) {
    setError(null);
    try {
      const next = await saveBrowserAttach(at, origins);
      if (!next) return false;
      setShown(next);
      setEndpoint(next.endpoint);
      return true;
    } catch (err) {
      setError(String(err));
      return false;
    }
  }

  async function add(event: FormEvent) {
    event.preventDefault();
    const wanted = site.trim();
    if (!wanted || !shown) return;
    if (await save([...shown.origins, wanted])) setSite("");
  }

  const problem = error ?? shown.problem;
  return (
    <div className="settings-group" role="group" aria-label="Your browser">
      <h2>Your browser</h2>
      <p className="note">{ATTACH_NOTE}</p>
      <label>
        Browser address
        <input
          value={endpoint}
          onChange={(e) => setEndpoint(e.target.value)}
          onBlur={() => {
            if (endpoint.trim() !== shown.endpoint) void save(shown.origins);
          }}
          spellCheck={false}
        />
      </label>
      {shown.origins.length > 0 && (
        <ul className="attach-sites" aria-label="Allowed sites">
          {shown.origins.map((origin) => (
            <li key={origin}>
              <span>{origin}</span>
              <button
                type="button"
                aria-label={`Remove ${origin}`}
                onClick={() => void save(shown.origins.filter((o) => o !== origin))}
              >
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}
      <form className="attach-add" onSubmit={(e) => void add(e)}>
        <label>
          Allow a site
          <input
            value={site}
            onChange={(e) => setSite(e.target.value)}
            placeholder="https://github.com"
            spellCheck={false}
          />
        </label>
        <button type="submit" disabled={!site.trim()}>
          Allow
        </button>
      </form>
      <p className="note">
        {shown.active
          ? `The browser tool works in your browser, on ${shown.origins.length === 1 ? "this site" : "these sites"} only.`
          : "The browser tool uses its own browser."}
      </p>
      {problem && <p className="note warn">{problem}</p>}
    </div>
  );
}
