import { useEffect, useRef, useState } from "react";
import {
  getSettings,
  grokSignOut,
  pollGrokSignIn,
  startGrokSignIn,
  type GrokSignInStart,
  type Settings,
  type SignedIn,
} from "./api";
import { openExternal } from "./external";
import { StatusLine, useNote } from "./ui";

export const SIGN_IN_NOTE =
  "Use your Grok subscription instead of API credits. Your sign-in is kept in this device's keychain.";

/** Who is signed in, in a few words. */
export function signedInAs(who: SignedIn): string {
  if (who.name && who.email) return `${who.name} (${who.email})`;
  return who.name ?? who.email ?? "your Grok account";
}

/** Settings → Account: Sign in with Grok by device code. The page shows the
 * code with Copy and Open x.ai, asks xAI every few seconds, and shows the
 * account once it says yes. `onChange` gets the settings after a change. */
export function GrokSignIn({
  signedIn,
  onChange,
}: {
  signedIn: SignedIn | null | undefined;
  onChange: (settings: Settings) => void;
}) {
  const [code, setCode] = useState<GrokSignInStart | null>(null);
  const [busy, setBusy] = useState(false);
  const { note, ok, fail, clear } = useNote();
  const timer = useRef<number | null>(null);

  function stop() {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = null;
  }
  useEffect(() => stop, []);

  function wait(seconds: number) {
    stop();
    timer.current = window.setTimeout(() => void poll(), seconds * 1000);
  }

  async function poll() {
    timer.current = null;
    try {
      const got = await pollGrokSignIn();
      if (got.status === "pending") wait(got.interval);
      else if (got.status === "failed") {
        setCode(null);
        fail(got.error);
      } else {
        setCode(null);
        onChange(await getSettings());
        ok(`Signed in as ${signedInAs(got.signedIn)}.`);
      }
    } catch (err) {
      setCode(null);
      fail(err);
    }
  }

  async function start() {
    clear();
    setBusy(true);
    try {
      const got = await startGrokSignIn();
      setCode(got);
      wait(got.interval);
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  }

  function cancel() {
    stop();
    setCode(null);
    clear();
  }

  async function signOut() {
    try {
      onChange(await grokSignOut());
      ok("Signed out of Grok.");
    } catch (err) {
      fail(err);
    }
  }

  async function copy(text: string) {
    try {
      await navigator.clipboard.writeText(text);
      ok("Code copied.");
    } catch {
      fail("Couldn't copy. Type the code instead.");
    }
  }

  return (
    <div className="grok-sign-in" role="region" aria-label="Sign in with Grok">
      <h2>Sign in with Grok</h2>
      {signedIn ? (
        <>
          <p>Signed in as {signedInAs(signedIn)}.</p>
          <button type="button" className="quiet" onClick={() => void signOut()}>
            Sign out
          </button>
        </>
      ) : code ? (
        <>
          <p>Open x.ai, sign in and type this code:</p>
          <p className="device-code" aria-label="Sign-in code">
            {code.userCode}
          </p>
          <div className="card-buttons">
            <button
              type="button"
              onClick={() =>
                void openExternal(code.verificationUriComplete ?? code.verificationUri).catch(fail)
              }
            >
              Open x.ai
            </button>
            <button type="button" className="quiet" onClick={() => void copy(code.userCode)}>
              Copy code
            </button>
            <button type="button" className="quiet" onClick={cancel}>
              Cancel
            </button>
          </div>
          <p className="note" role="status">
            Waiting for you to approve it on x.ai…
          </p>
        </>
      ) : (
        <>
          <p className="note">{SIGN_IN_NOTE}</p>
          <button type="button" disabled={busy} onClick={() => void start()}>
            Sign in with Grok
          </button>
        </>
      )}
      <StatusLine note={note} />
    </div>
  );
}
