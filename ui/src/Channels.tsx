import { useEffect, useState, type FormEvent } from "react";
import {
  addChannelWebhook,
  approveChannelCode,
  connectChannel,
  disconnectChannel,
  getChannels,
  removeChannelWebhook,
  setChannelAsks,
  telegramQr,
  unpairChannelSender,
  type ChannelKind,
  type ChannelState,
  type Channels as Shape,
  type StartQr,
} from "./channelsApi";
import { PRODUCT_NAME } from "./brand";
import { StatusLine, useNote } from "./ui";

export const CHANNELS_NOTE = `Message ${PRODUCT_NAME} from your phone through a bot you made. Anyone else who messages the bot gets a pairing code and nothing more, until you enter that code here.`;

export const WEBHOOKS_NOTE = `A service you use (a deploy, a form, a store) can trigger ${PRODUCT_NAME} with a signed POST. It sends X-Webhook-Timestamp (Unix seconds) and X-Webhook-Signature: sha256= then the HMAC-SHA256 of "timestamp.body" with the secret. The hub listens while this computer is shared on the Devices page.`;

export const QR_NOTE =
  "Scan it with your phone's camera. Telegram opens the bot and pairs that phone, with no code to type. It works once, for 10 minutes, and showing a new one turns this one off.";

function statusOf(ch: ChannelState): string {
  if (!ch.live) return "Coming next";
  if (!ch.connected) return "Not connected";
  return ch.running ? "Listening for messages" : "Connected, not listening";
}

/** Connectors → Phone and chat apps. Hidden in the web app. */
export function Channels() {
  const [shown, setShown] = useState<Shape | null>(null);
  const [tokens, setTokens] = useState<Partial<Record<ChannelKind, string>>>({});
  const [code, setCode] = useState("");
  const [hookName, setHookName] = useState("");
  const [secret, setSecret] = useState<string | null>(null);
  const [qr, setQr] = useState<StartQr | null>(null);
  const [busy, setBusy] = useState(false);
  const { note, ok, fail } = useNote();

  useEffect(() => {
    getChannels().then(setShown, () => setShown(null));
  }, []);

  if (!shown) return null;
  // Names come from the shell's channel table, so a new kind needs no code here.
  const names = new Map(shown.channels.map((ch) => [ch.kind, ch.name]));
  const nameOf = (kind: ChannelKind) => names.get(kind) ?? kind;

  async function act(step: () => Promise<Shape | null>): Promise<boolean> {
    setBusy(true);
    try {
      const next = await step();
      if (!next) return false;
      setShown(next);
      setSecret(next.newSecret);
      ok(next.status);
      return true;
    } catch (err) {
      fail(err);
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function showQr() {
    setBusy(true);
    try {
      setQr(await telegramQr());
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  }

  async function connect(event: FormEvent, kind: ChannelKind) {
    event.preventDefault();
    const token = (tokens[kind] ?? "").trim();
    // The token leaves the page's state as soon as it is sent.
    setTokens((t) => ({ ...t, [kind]: "" }));
    if (token) await act(() => connectChannel(kind, token));
  }

  async function addHook(event: FormEvent) {
    event.preventDefault();
    if (await act(() => addChannelWebhook(hookName.trim()))) setHookName("");
  }

  async function approve(event: FormEvent) {
    event.preventDefault();
    if (await act(() => approveChannelCode(code.trim()))) setCode("");
  }

  return (
    <section className="channels" aria-label="Phone and chat apps">
      <h2>Phone and chat apps</h2>
      <p className="note">{CHANNELS_NOTE}</p>
      <ul className="rows">
        {shown.channels.map((ch) => (
          <li key={ch.kind} className="card" aria-label={ch.name}>
            <div className="card-title">{ch.name}</div>
            <div className="card-status">
              {statusOf(ch)}
            </div>
            {ch.problem && <div className="card-health">{ch.problem}</div>}
            {!ch.live ? null : ch.connected ? (
              <>
                <div className="card-buttons">
                  {ch.kind === "telegram" && (
                    <button type="button" disabled={busy} onClick={() => void showQr()}>
                      {qr ? "Show a new QR code" : "Pair a phone with a QR code"}
                    </button>
                  )}
                  <button
                    type="button"
                    className="quiet danger"
                    disabled={busy}
                    onClick={() => {
                      setQr(null);
                      void act(() => disconnectChannel(ch.kind));
                    }}
                  >
                    Disconnect
                  </button>
                </div>
                {ch.kind === "telegram" && qr && (
                  <figure className="channel-qr" aria-label="Telegram pairing QR code">
                    <img
                      src={`data:image/svg+xml;charset=utf-8,${encodeURIComponent(qr.svg)}`}
                      alt="QR code that opens the bot in Telegram and pairs your phone"
                      width={240}
                      height={240}
                    />
                    <figcaption className="note">
                      {QR_NOTE} On the phone itself, open <code>{qr.link}</code>
                    </figcaption>
                    <button type="button" className="quiet" onClick={() => setQr(null)}>
                      Hide QR code
                    </button>
                  </figure>
                )}
              </>
            ) : (
              <form className="card-buttons" onSubmit={(e) => void connect(e, ch.kind)}>
                <label>
                  {ch.setupLabel}
                  <input
                    type="password"
                    autoComplete="off"
                    spellCheck={false}
                    value={tokens[ch.kind] ?? ""}
                    placeholder={ch.setupHint}
                    onChange={(e) => setTokens((t) => ({ ...t, [ch.kind]: e.target.value }))}
                  />
                </label>
                <button type="submit" disabled={busy || !(tokens[ch.kind] ?? "").trim()}>
                  Connect
                </button>
              </form>
            )}
          </li>
        ))}
      </ul>
      {shown.pending.length > 0 && (
        <ul className="rows" aria-label="Waiting to pair">
          {shown.pending.map((p) => (
            <li key={`${p.channel}-${p.code}`} className="card-status">
              {p.label || "Someone"} on {nameOf(p.channel)} was sent a code
            </li>
          ))}
        </ul>
      )}
      <form className="card-buttons" onSubmit={(e) => void approve(e)}>
        <label>
          Pairing code
          <input value={code} onChange={(e) => setCode(e.target.value)} spellCheck={false} autoComplete="off" />
        </label>
        <button type="submit" disabled={busy || !code.trim()}>
          Pair
        </button>
      </form>
      {shown.paired.length > 0 && (
        <ul className="rows" aria-label="Paired senders">
          {shown.paired.map((p) => (
            <li key={`${p.channel}-${p.sender}`} className="card-buttons">
              <span>
                {nameOf(p.channel)} {p.sender}
                {p.asks && !p.reachable && <span className="note"> · asks start once they write to the bot</span>}
              </span>
              <button
                type="button"
                className="quiet"
                disabled={busy}
                aria-pressed={p.asks}
                aria-label={`Approval asks for ${p.sender} on ${nameOf(p.channel)}`}
                onClick={() => void act(() => setChannelAsks(p.channel, p.sender, !p.asks))}
              >
                {p.asks ? "Stop approval asks" : "Send approval asks"}
              </button>
              <button
                type="button"
                className="quiet"
                disabled={busy}
                aria-label={`Unpair ${p.sender} on ${nameOf(p.channel)}`}
                onClick={() => void act(() => unpairChannelSender(p.channel, p.sender))}
              >
                Unpair
              </button>
            </li>
          ))}
        </ul>
      )}
      <h3>Webhooks</h3>
      <p className="note">{WEBHOOKS_NOTE}</p>
      {shown.webhooks.length > 0 && (
        <ul className="rows" aria-label="Webhooks">
          {shown.webhooks.map((h) => (
            <li key={h.name} className="card-buttons">
              <span>
                {h.name} <code>POST {h.path}</code>
              </span>
              <button
                type="button"
                className="quiet danger"
                disabled={busy}
                aria-label={`Remove webhook ${h.name}`}
                onClick={() => void act(() => removeChannelWebhook(h.name))}
              >
                Remove
              </button>
            </li>
          ))}
        </ul>
      )}
      {secret && (
        <p className="note warn" role="group" aria-label="New webhook secret">
          Secret: <code>{secret}</code>
        </p>
      )}
      <form className="card-buttons" onSubmit={(e) => void addHook(e)}>
        <label>
          Webhook name
          <input
            value={hookName}
            onChange={(e) => setHookName(e.target.value)}
            placeholder="deploys"
            spellCheck={false}
            autoComplete="off"
          />
        </label>
        <button type="submit" disabled={busy || !hookName.trim()}>
          Add webhook
        </button>
      </form>
      {shown.recent.length > 0 && (
        <>
          <h3>Recent answers</h3>
          <ul className="rows" aria-label="Recent answers">
            {shown.recent.map((r) => (
              <li key={`${r.doneMs}-${r.source}-${r.from}`}>
                <strong>
                  {r.source} · {r.from}
                </strong>
                {r.text && <p className="note">{r.text}</p>}
                {r.problem ? <p className="note warn">{r.problem}</p> : <p>{r.answer}</p>}
              </li>
            ))}
          </ul>
        </>
      )}
      <StatusLine note={note} />
    </section>
  );
}
