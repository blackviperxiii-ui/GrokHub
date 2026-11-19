import { useCallback, useEffect, useEffectEvent, useRef, useState } from "react";
import {
  addAiProvider,
  allowAiProvider,
  answerAiModelsCard,
  getAiModels,
  openRouterSignInStatus,
  removeAiProvider,
  revokeAiPremium,
  saveAiSpend,
  saveBraveKey,
  saveWebSearch,
  startOpenRouterSignIn,
  stopOpenRouterSignIn,
  unbenchAiModel,
  waitOpenRouterSignIn,
  type AiModelsCard,
  type AiModelsView,
  type AiWebSearch,
  type OpenRouterSignIn,
  type OpenRouterSignInStatus,
} from "./api";
import { ConfirmRow, StatusLine, useNote } from "./ui";
import { ClaudePlanCard } from "./ClaudePlan";

export const KEY_NOTE = "The key is saved to your keyring after you approve the card. It is never shown again.";
export const ALLOW_NOTE =
  "Your chats and memory would go to this provider. Allow keeps it until you revoke it in Settings → Permissions.";
export const SPEND_NOTE =
  "Models over your price limit ask once before Auto uses them. Server responses govern rate and quota limits.";
export const LOCAL_NOTE = "Models that run on this PC, and whether each task gets its own model, are set in Local AI.";
export const PREMIUM_NONE = "None. A model over your price limit asks once before Auto uses it.";
export const BENCHED_NONE = "None. You can tell chat to stop using a model or provider at any time.";
export const WEB_SEARCH_NOTE =
  "For OpenRouter and the local model. Free DuckDuckGo Lite by default, no account. Grok still uses xAI's own search.";
export const BACKEND_HINT = "Default stays free. SearXNG needs your instance URL. Brave needs a key in your keyring.";
export const SEARXNG_NOTE =
  "Your SearXNG server can be on the internet, this computer (http://localhost:8888) or your own network (http://192.168.1.5:8888). It must allow JSON results (?format=json).";
export const BRAVE_SET = "A Brave Search key is in your keyring. Paste a new one to replace it, or remove it.";
export const OPENROUTER_NOTE = "No key to paste: sign in on OpenRouter's page and it hands over a new key.";
export const OPENROUTER_WAIT = "Waiting for OpenRouter. Finish signing in in your browser.";
export const OPENROUTER_KEY_WAIT = "OpenRouter sent a key. Save it on the card below.";
export const OPENROUTER_BY_HAND = "Your browser didn't open. Open this link to sign in:";
export const OPENROUTER_REOPEN = "No OpenRouter tab, or closed it? Open this link:";
export const OPENROUTER_STOPPED = "Sign-in stopped. Nothing was saved.";
/** The shell's wait ends with these when the page stopped it, or a newer page took it over. */
const STOPPED = "OpenRouter sign-in stopped";
const WAITED_ELSEWHERE = "This sign-in moved to a newer page.";
export const BRAVE_UNSET = "Paste a Brave Search API key. It stays in your keyring, never in app.json.";

/** Settings → AI models → Web search: the backend, a SearXNG URL, a Brave key. */
function WebSearch({
  search,
  busy,
  run,
}: {
  search: AiWebSearch;
  busy: boolean;
  run: (step: () => Promise<AiModelsView>) => Promise<boolean>;
}) {
  const [url, setUrl] = useState(search.searxngUrl);
  const [braveKey, setBraveKey] = useState("");
  const [removingKey, setRemovingKey] = useState(false);

  async function saveUrl() {
    const sent = url.trim();
    if (await run(() => saveWebSearch("searxng", sent))) setUrl(sent);
  }

  function saveKey() {
    const sent = braveKey;
    // The field is empty the moment the key leaves it, win or lose.
    setBraveKey("");
    void run(() => saveBraveKey(sent));
  }

  return (
    <>
      <h3>Web search</h3>
      <p className="note">{WEB_SEARCH_NOTE}</p>
      <div className="card-buttons">
        <label>
          Search backend
          <select
            aria-label="Search backend"
            value={search.backend}
            disabled={busy}
            onChange={(e) => {
              setRemovingKey(false);
              void run(() => saveWebSearch(e.target.value, search.searxngUrl));
            }}
          >
            {search.backends.map((b) => (
              <option key={b.id} value={b.id}>
                {b.label}
              </option>
            ))}
          </select>
        </label>
      </div>
      <p className="note">{BACKEND_HINT}</p>
      {search.backend === "searxng" && (
        <>
          <form
            className="card-buttons"
            onSubmit={(e) => {
              e.preventDefault();
              void saveUrl();
            }}
          >
            <input
              aria-label="SearXNG URL"
              placeholder="https://searx.example"
              value={url}
              disabled={busy}
              onChange={(e) => setUrl(e.target.value)}
            />
            <button type="submit" disabled={busy || url.trim() === search.searxngUrl}>
              Save URL
            </button>
          </form>
          {search.searxngProblem && <p className="note warn">{search.searxngProblem}</p>}
          <p className="note">{SEARXNG_NOTE}</p>
        </>
      )}
      {search.backend === "brave" && (
        <>
          <form
            className="card-buttons"
            onSubmit={(e) => {
              e.preventDefault();
              saveKey();
            }}
          >
            <input
              type="password"
              autoComplete="off"
              aria-label="Brave API key"
              placeholder="API key"
              value={braveKey}
              onChange={(e) => setBraveKey(e.target.value)}
            />
            <button type="submit" disabled={busy || !braveKey.trim()}>
              Save key
            </button>
            {search.braveKeySet && (
              <button type="button" className="quiet" disabled={busy} onClick={() => setRemovingKey(true)}>
                Remove key
              </button>
            )}
          </form>
          {removingKey && (
            <ConfirmRow
              question="Remove the Brave Search key?"
              detail="It leaves the keyring. Brave searches stop until you paste a key again."
              action="Remove"
              busy={busy}
              onConfirm={() => {
                setRemovingKey(false);
                void run(() => saveBraveKey(""));
              }}
              onCancel={() => setRemovingKey(false)}
            />
          )}
          <p className="note">{search.braveKeySet ? BRAVE_SET : BRAVE_UNSET}</p>
        </>
      )}
    </>
  );
}

export function capLabel(usd: number): string {
  return usd === 0 ? "Off" : `$${usd} a week`;
}

export function ceilingLabel(usd: number): string {
  return `$${usd} per million`;
}

function quality(q: number | null): string {
  return q === null ? "unscored" : `${Math.round(q * 100)}%`;
}

/** Deny a card nobody is left to answer, so the app drops the key now and not after its wait. */
function deny(id: string) {
  void (async () => {
    try {
      await answerAiModelsCard(id, false);
    } catch {
      // The page is gone: there is nowhere to show it, and the card's own wait still ends it.
    }
  })();
}

/** Settings → AI models: other providers, the routing table, spend limits. */
export function AiModels() {
  const [view, setView] = useState<AiModelsView | null>(null);
  const [baseUrl, setBaseUrl] = useState("");
  const [key, setKey] = useState("");
  const [card, setCard] = useState<AiModelsCard | null>(null);
  const [removing, setRemoving] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [signIn, setSignIn] = useState<OpenRouterSignIn | null>(null);
  const { note, ok, fail } = useNote();
  // The card on screen, for the moment the page is left.
  const openCard = useRef<string | null>(null);
  // A browser sign-in this page waits for.
  const signingIn = useRef(false);
  // The newest follow(): only it may end the waiting state on this page.
  const followed = useRef(0);
  const gone = useRef(false);

  // Settings unmounts this page when another section is picked. Leaving is a
  // Deny, and stops a sign-in so its key never lands on a card nobody sees.
  useEffect(() => {
    gone.current = false;
    return () => {
      gone.current = true;
      const id = openCard.current;
      openCard.current = null;
      if (id) deny(id);
      if (signingIn.current) {
        signingIn.current = false;
        stopOpenRouterSignIn().catch(() => undefined);
      }
    };
  }, []);

  /** False when the page was left while the card was on its way. */
  function show(next: AiModelsCard): boolean {
    if (gone.current) {
      deny(next.id);
      return false;
    }
    openCard.current = next.id;
    setCard(next);
    ok(null);
    return true;
  }

  const apply = useCallback(
    (next: AiModelsView) => {
      setView(next);
      ok(next.status);
    },
    [ok],
  );

  useEffect(() => {
    let live = true;
    getAiModels().then(
      (next) => {
        if (live) setView(next);
      },
      (err) => {
        if (live) fail(err);
      },
    );
    return () => {
      live = false;
    };
  }, [fail]);

  /** True when the step went through. */
  async function run(step: () => Promise<AiModelsView>): Promise<boolean> {
    setBusy(true);
    try {
      apply(await step());
      return true;
    } catch (err) {
      fail(err);
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function startAdd() {
    const sent = key;
    // The field is empty the moment the key leaves it, win or lose.
    setKey("");
    setBusy(true);
    try {
      show(await addAiProvider(baseUrl, sent));
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  }

  async function startSignIn() {
    setBusy(true);
    let started: OpenRouterSignIn;
    try {
      started = await startOpenRouterSignIn();
    } catch (err) {
      fail(err);
      return;
    } finally {
      setBusy(false);
    }
    await follow(started);
  }

  /** Wait for a sign-in that reached the browser, then show its key card. */
  async function follow(started: OpenRouterSignIn) {
    if (gone.current) {
      stopOpenRouterSignIn().catch(() => undefined);
      return;
    }
    const me = ++followed.current;
    signingIn.current = true;
    setSignIn(started);
    ok(null);
    let next: AiModelsCard;
    try {
      next = await waitOpenRouterSignIn();
    } catch (err) {
      // A newer wait on this page has the sign-in now, and keeps its state.
      if (me !== followed.current) return;
      signingIn.current = false;
      if (gone.current) return;
      setSignIn(null);
      // Another page has it now: nothing to say here.
      if (String(err) === WAITED_ELSEWHERE) return;
      if (String(err) === STOPPED) ok(OPENROUTER_STOPPED);
      else fail(err);
      return;
    }
    signingIn.current = false;
    if (!gone.current) setSignIn(null);
    if (show(next)) ok(OPENROUTER_KEY_WAIT);
  }

  function stopSignIn() {
    stopOpenRouterSignIn().catch(fail);
  }

  // A page that was reloaded mid sign-in picks it up again: the sign-in that
  // still waits, or the key card it never got to show.
  const pickUp = useEffectEvent((found: OpenRouterSignInStatus) => {
    if (found.card) {
      if (show(found.card)) ok(OPENROUTER_KEY_WAIT);
    } else if (found.waiting) {
      void follow(found.waiting);
    }
  });
  useEffect(() => {
    let live = true;
    openRouterSignInStatus().then(
      (found) => {
        if (live) pickUp(found);
      },
      () => undefined,
    );
    return () => {
      live = false;
    };
  }, []);

  async function copyLink(url: string) {
    try {
      await navigator.clipboard.writeText(url);
      ok("Link copied.");
    } catch {
      fail("Couldn't copy. Select the link and copy it.");
    }
  }

  async function startAllow(id: string) {
    setBusy(true);
    try {
      show(await allowAiProvider(id));
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  }

  async function answer(approve: boolean) {
    if (!card) return;
    const id = card.id;
    const wasKey = card.kind === "key";
    openCard.current = null;
    setCard(null);
    const done = await run(() => answerAiModelsCard(id, approve));
    // A key that wasn't saved keeps its address, so only the key is pasted again.
    if (wasKey && approve && done) setBaseUrl("");
  }

  if (!view) {
    return (
      <section className="ai-models" aria-label="AI models">
        <StatusLine note={note} />
      </section>
    );
  }

  const { spend } = view;
  return (
    <section className="ai-models" aria-label="AI models">
      <StatusLine note={note} />
      {view.locked && <p className="status status--error">{view.locked}</p>}

      <h3>Claude (your plan)</h3>
      <ClaudePlanCard />

      <h3>Other AI providers</h3>
      <p className="note">
        Bring your own key for OpenRouter, Anthropic or another provider. A provider stays off until you add its key and allow where chats go.
      </p>
      {card && (
        <div className="card-buttons confirm-row" role="group" aria-label={card.action}>
          <span>
            {card.action}? {card.kind === "key" ? KEY_NOTE : ALLOW_NOTE}
          </span>
          <button type="button" onClick={() => void answer(true)}>
            {card.kind === "key" ? "Save" : "Allow"}
          </button>
          <button type="button" className="quiet" onClick={() => void answer(false)}>
            Deny
          </button>
        </div>
      )}
      <ul className="rows" aria-label="Providers">
        {view.providers.length === 0 && <li className="empty">No other providers yet.</li>}
        {view.providers.map((p) => (
          <li key={p.id} className="card" aria-label={p.id}>
            <div className="card-title">{p.id}</div>
            <div className="card-status">
              {p.kind} · {p.dest} · <span className="badge">{p.state}</span>
            </div>
            <div className="card-buttons">
              {p.hasKey && !p.ready && (
                <button type="button" disabled={busy || card !== null || signIn !== null} onClick={() => void startAllow(p.id)}>
                  Allow {p.dest}
                </button>
              )}
              <button type="button" className="quiet" disabled={busy} onClick={() => setRemoving(p.id)}>
                Remove
              </button>
            </div>
            {removing === p.id && (
              <ConfirmRow
                question={`Remove ${p.id}?`}
                detail="Its key leaves the keyring. Its permission stays until you revoke it in Settings → Permissions."
                action="Remove"
                busy={busy}
                onConfirm={() => {
                  setRemoving(null);
                  void run(() => removeAiProvider(p.id));
                }}
                onCancel={() => setRemoving(null)}
              />
            )}
          </li>
        ))}
      </ul>
      <div className="card-buttons">
        <button
          type="button"
          disabled={busy || card !== null || signIn !== null}
          onClick={() => void startSignIn()}
        >
          {view.providers.some((p) => p.id === "openrouter") ? "Sign in to OpenRouter again" : "Sign in with OpenRouter"}
        </button>
        <span className="note">{OPENROUTER_NOTE}</span>
      </div>
      {signIn && (
        <div className="card-buttons">
          <span>{OPENROUTER_WAIT}</span>
          <button type="button" className="quiet" onClick={stopSignIn}>
            Stop signing in
          </button>
        </div>
      )}
      {signIn && signIn.url && (
        <div className="card-buttons" role="group" aria-label="Open OpenRouter by hand">
          <span>
            {signIn.opened ? OPENROUTER_REOPEN : OPENROUTER_BY_HAND}
            {!signIn.opened && signIn.why ? ` (${signIn.why})` : ""}
          </span>
          <input readOnly aria-label="OpenRouter sign-in link" value={signIn.url} onFocus={(e) => e.target.select()} />
          <button type="button" className="quiet" onClick={() => void copyLink(signIn.url)}>
            Copy link
          </button>
        </div>
      )}
      <div className="card-buttons">
        <input
          aria-label="Provider address"
          placeholder="https://openrouter.ai/api/v1"
          value={baseUrl}
          onChange={(e) => setBaseUrl(e.target.value)}
        />
        <input
          type="password"
          autoComplete="off"
          aria-label="Provider API key"
          placeholder="API key"
          value={key}
          onChange={(e) => setKey(e.target.value)}
        />
        <button
          type="button"
          disabled={busy || card !== null || signIn !== null || !baseUrl.trim() || !key.trim()}
          onClick={() => void startAdd()}
        >
          Add provider
        </button>
      </div>

      <WebSearch search={view.webSearch} busy={busy} run={run} />

      <h3>Spend limits</h3>
      <p className="note">{SPEND_NOTE}</p>
      <div className="card-buttons">
        <label>
          Price limit
          <select
            aria-label="Price limit"
            value={spend.priceCeilingUsdPerM}
            disabled={busy}
            onChange={(e) => void run(() => saveAiSpend(0, Number(e.target.value)))}
          >
            {spend.ceilings.map((c) => (
              <option key={c} value={c}>
                {ceilingLabel(c)}
              </option>
            ))}
          </select>
        </label>
      </div>

      <h3>Benched models</h3>
      <ul className="rows" aria-label="Benched models">
        {view.benched.length === 0 && <li className="empty">{BENCHED_NONE}</li>}
        {view.benched.map((b) => (
          <li key={b.id} className="card" aria-label={b.label}>
            <div className="card-title">{b.label}</div>
            <div className="card-buttons">
              <button type="button" className="quiet" disabled={busy} onClick={() => void run(() => unbenchAiModel(b.id))}>
                Unbench
              </button>
            </div>
          </li>
        ))}
      </ul>

      <h3>Paid upgrades</h3>
      <ul className="rows" aria-label="Paid upgrades">
        {view.premium.length === 0 && <li className="empty">{PREMIUM_NONE}</li>}
        {view.premium.map((g) => (
          <li key={g.id} className="card" aria-label={g.label}>
            <div className="card-title">{g.label}</div>
            <div className="card-buttons">
              <button type="button" className="quiet" disabled={busy} onClick={() => void run(() => revokeAiPremium(g.id))}>
                Revoke
              </button>
            </div>
          </li>
        ))}
      </ul>

      <h3>Routing table</h3>
      <p className="note">{LOCAL_NOTE}</p>
      {view.table.length === 0 ? (
        <p className="empty">No routing table yet. It builds after the first few chats.</p>
      ) : (
        view.table.map((c) => (
          <div key={c.class} className="card" aria-label={`${c.class} routing`}>
            <div className="card-title">{c.class}</div>
            <ol>
              {c.rows.map((r) => (
                <li key={r.model}>
                  {r.model}
                  {r.effort ? ` (${r.effort})` : ""} · quality {quality(r.quality)}
                  {r.why ? ` · ${r.why}` : ""}
                </li>
              ))}
            </ol>
          </div>
        ))
      )}
    </section>
  );
}
