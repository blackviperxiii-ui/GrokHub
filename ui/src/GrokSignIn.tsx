import { useEffect, useEffectEvent, useRef, useState } from "react";
import {
  getSettings,
  grokSignOut,
  pollGrokSignIn,
  startGrokSignIn,
  type GrokSignInPoll,
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

/** The shell's answer to a poll when it holds no code (`NOT_STARTED` in
 * `xai_keychain.rs`): there is nothing left to wait for. */
const NO_CODE = "Start Sign in with Grok first.";

/** Whether a code good until `deadline` (ms on this computer's clock) ran out. */
function ranOut(deadline: number): boolean {
  return Date.now() >= deadline;
}

/** Settings → Account: Sign in with Grok by device code. The page shows the
 * code with Copy and Open x.ai, asks xAI every few seconds, and shows the
 * account once it says yes. A poll that fails leaves the code up and is asked
 * again, until the code's own time is up. `onChange` gets the settings after
 * a change. */
export function GrokSignIn({
  signedIn,
  onChange,
}: {
  signedIn: SignedIn | null | undefined;
  onChange: (settings: Settings) => void;
}) {
  const [code, setCode] = useState<GrokSignInStart | null>(null);
  const [busy, setBusy] = useState(false);
  // Why the last poll failed, while the page is still asking.
  const [trouble, setTrouble] = useState<string | null>(null);
  const { note, ok, fail, clear } = useNote();
  const timer = useRef<number | null>(null);
  // The last interval xAI asked for (slow_down raises it), whether a poll is
  // waiting on its answer, and whether the page is shown with a code to wait on.
  const interval = useRef(0);
  const polling = useRef(false);
  const live = useRef(false);
  // When the code on screen stops being good, by this computer's clock.
  const deadline = useRef(0);
  // Which code is on screen, counted from the first, so a poll that answers
  // late knows the code it asked about is gone.
  const round = useRef(0);

  function stop() {
    if (timer.current !== null) window.clearTimeout(timer.current);
    timer.current = null;
  }

  function wait(seconds: number) {
    stop();
    interval.current = seconds;
    if (live.current) timer.current = window.setTimeout(() => void poll(), seconds * 1000);
  }

  function expire() {
    setCode(null);
    fail("The code ran out. Start again.");
  }

  async function poll() {
    timer.current = null;
    if (ranOut(deadline.current)) {
      expire();
      return;
    }
    const asked = round.current;
    polling.current = true;
    let got: GrokSignInPoll;
    try {
      got = await pollGrokSignIn();
    } catch (err) {
      // The shell drops the code only with a "failed" answer. A poll that
      // rejects (no answer from xAI, say) left it held, so ask again.
      const why = String(err);
      if (asked !== round.current) {
        // An earlier code's poll: it says nothing about the one on screen.
        wait(interval.current);
      } else if (why === NO_CODE) {
        setCode(null);
        fail(why);
      } else {
        setTrouble(why);
        wait(interval.current);
      }
      return;
    } finally {
      polling.current = false;
    }
    if (got.status === "pending") {
      setTrouble(null);
      wait(got.interval);
    } else if (got.status === "failed") {
      setCode(null);
      fail(got.error);
    } else {
      setCode(null);
      try {
        onChange(await getSettings());
        ok(`Signed in as ${signedInAs(got.signedIn)}.`);
      } catch (err) {
        fail(err);
      }
    }
  }

  // Settings sits in an <Activity>: hiding it runs this cleanup and showing it
  // runs the effect again, so a code still on screen goes back to waiting. A
  // poll still out re-arms itself when it answers. A code that ran out while
  // the page was hidden goes at once.
  const resume = useEffectEvent(() => {
    if (ranOut(deadline.current)) expire();
    else if (!polling.current) wait(interval.current);
  });
  useEffect(() => {
    live.current = code !== null;
    if (code) resume();
    return () => {
      live.current = false;
      stop();
    };
  }, [code]);

  async function start() {
    clear();
    setBusy(true);
    try {
      const got = await startGrokSignIn();
      // Never 0 while a code is up: a poll still out waits this long too.
      interval.current = got.interval;
      deadline.current = Date.now() + got.expiresIn * 1000;
      round.current += 1;
      setTrouble(null);
      setCode(got);
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
            {trouble
              ? `Still waiting for you to approve it on x.ai. The last check failed: ${trouble}`
              : "Waiting for you to approve it on x.ai…"}
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
