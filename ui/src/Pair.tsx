import { useEffect, useEffectEvent, useState, type FormEvent } from "react";
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

/** Whether the address carries a pairing link's code (read only; `takeCodeFromLink` takes it). */
function linkHasCode(): boolean {
  return /(?:^#|&)code=[^&]/.test(window.location.hash);
}

/** The web app's first screen: pair this browser with the desktop. */
export function Pair({ onPaired }: { onPaired: () => void }) {
  const [code, setCode] = useState("");
  // A pairing link starts connecting as the screen opens, so it starts busy.
  const [busy, setBusy] = useState(linkHasCode);
  const [note, setNote] = useState<string | null>(null);

  function attempt(value: string): Promise<void> {
    return pair(value)
      .then(() => onPaired())
      .catch((err: unknown) => setNote(err instanceof Error ? err.message : String(err)))
      .finally(() => setBusy(false));
  }

  function connect(value: string) {
    setBusy(true);
    setNote(null);
    void attempt(value);
  }

  // Only once, on the first render: the link's code is gone after it is taken.
  const pairFromLink = useEffectEvent((value: string) => void attempt(value));
  useEffect(() => {
    const fromLink = takeCodeFromLink();
    if (fromLink) pairFromLink(fromLink);
  }, []);

  function submit(event: FormEvent) {
    event.preventDefault();
    connect(code);
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
