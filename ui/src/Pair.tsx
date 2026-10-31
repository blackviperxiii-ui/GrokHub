import { useCallback, useEffect, useState, type FormEvent } from "react";
import { App } from "./App";
import { inShell, onPairChange, pair, paired } from "./transport";
import { LOGO_MARK_URL, PRODUCT_NAME } from "./brand";

export const PAIR_HELP =
  `Open ${PRODUCT_NAME} on your computer, turn on Settings → Web access, and enter the pairing code it shows.`;

/**
 * Takes `#code=…` off the address (GrokHub's pairing link) so the code
 * doesn't stay in the address bar or the history.
 */
export function takeCodeFromLink(): string {
  const match = /(?:^#|&)code=([^&]*)/.exec(window.location.hash);
  if (!match) return "";
  const { pathname, search } = window.location;
  window.history.replaceState(null, "", pathname + search);
  return decodeURIComponent(match[1]);
}

/** The web app's first screen: pair this browser with the desktop. */
export function Pair({ onPaired }: { onPaired: () => void }) {
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);

  const connect = useCallback(
    async (value: string) => {
      setBusy(true);
      setNote(null);
      try {
        await pair(value);
        onPaired();
      } catch (err) {
        setNote(err instanceof Error ? err.message : String(err));
      } finally {
        setBusy(false);
      }
    },
    [onPaired],
  );

  useEffect(() => {
    // The link's code is taken off the address, so a re-run finds nothing.
    const fromLink = takeCodeFromLink();
    if (fromLink) void connect(fromLink);
  }, [connect]);

  function submit(event: FormEvent) {
    event.preventDefault();
    void connect(code);
  }

  return (
    <section className="pair" aria-label="Pair">
      <h1 className="brand-lockup">
        <img src={LOGO_MARK_URL} alt="" />
        {PRODUCT_NAME}
      </h1>
      <p>{PAIR_HELP}</p>
      <form onSubmit={submit}>
        <label>
          Pairing code
          <input
            type="password"
            autoComplete="off"
            value={code}
            onChange={(e) => setCode(e.target.value)}
          />
        </label>
        <button type="submit" disabled={busy || !code.trim()}>
          {busy ? "Connecting…" : "Connect"}
        </button>
      </form>
      {note && (
        <p className="error" role="alert">
          {note}
        </p>
      )}
    </section>
  );
}

/** The shell opens the app; a browser pairs first, and again if dropped. */
export function Root() {
  const [linked, setLinked] = useState(() => inShell() || paired());
  useEffect(() => (inShell() ? undefined : onPairChange(setLinked)), []);
  return linked ? <App /> : <Pair onPaired={() => setLinked(true)} />;
}
