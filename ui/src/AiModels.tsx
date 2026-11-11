import { useCallback, useEffect, useRef, useState } from "react";
import {
  addAiProvider,
  allowAiProvider,
  answerAiModelsCard,
  getAiModels,
  removeAiProvider,
  revokeAiPremium,
  saveAiSpend,
  type AiModelsCard,
  type AiModelsView,
} from "./api";
import { ConfirmRow, StatusLine, useNote } from "./ui";

export const KEY_NOTE = "The key is saved to your keyring after you approve the card. It is never shown again.";
export const ALLOW_NOTE =
  "Your chats and memory would go to this provider. Allow keeps it until you revoke it in Settings → Permissions.";
export const SPEND_NOTE =
  "API key only. At 80% background work moves to cheaper settings. At 100% it pauses and chats ask first. These are limits; nothing here spends money.";
export const LOCAL_NOTE = "Models that run on this PC, and whether each task gets its own model, are set in Local AI.";
export const PREMIUM_NONE = "None. A model over your price limit asks once before Auto uses it.";

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
  const { note, ok, fail } = useNote();
  // The card on screen, for the moment the page is left.
  const openCard = useRef<string | null>(null);
  const gone = useRef(false);

  // Settings unmounts this page when another section is picked. Leaving is a Deny.
  useEffect(() => {
    gone.current = false;
    return () => {
      gone.current = true;
      const id = openCard.current;
      openCard.current = null;
      if (id) deny(id);
    };
  }, []);

  function show(next: AiModelsCard) {
    // The page was left while the card was on its way.
    if (gone.current) {
      deny(next.id);
      return;
    }
    openCard.current = next.id;
    setCard(next);
    ok(null);
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
                <button type="button" disabled={busy || card !== null} onClick={() => void startAllow(p.id)}>
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
          disabled={busy || card !== null || !baseUrl.trim() || !key.trim()}
          onClick={() => void startAdd()}
        >
          Add provider
        </button>
      </div>

      <h3>Spend limits</h3>
      <p className="note">{SPEND_NOTE}</p>
      <div className="card-buttons">
        <label>
          Weekly spend cap
          <select
            aria-label="Weekly spend cap"
            value={spend.weeklyCapUsd}
            disabled={busy}
            onChange={(e) => void run(() => saveAiSpend(Number(e.target.value), spend.priceCeilingUsdPerM))}
          >
            {spend.caps.map((c) => (
              <option key={c} value={c}>
                {capLabel(c)}
              </option>
            ))}
          </select>
        </label>
        <label>
          Price limit
          <select
            aria-label="Price limit"
            value={spend.priceCeilingUsdPerM}
            disabled={busy}
            onChange={(e) => void run(() => saveAiSpend(spend.weeklyCapUsd, Number(e.target.value)))}
          >
            {spend.ceilings.map((c) => (
              <option key={c} value={c}>
                {ceilingLabel(c)}
              </option>
            ))}
          </select>
        </label>
      </div>

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
