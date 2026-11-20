import { useEffect, useState, type FormEvent } from "react";
import {
  approveChannelCode,
  connectChannel,
  disconnectChannel,
  getChannels,
  unpairChannelSender,
  type ChannelKind,
  type Channels as Shape,
} from "./channelsApi";
import { PRODUCT_NAME } from "./brand";
import { StatusLine, useNote } from "./ui";

const NAMES: Record<ChannelKind, string> = { telegram: "Telegram", discord: "Discord" };

export const CHANNELS_NOTE = `Message ${PRODUCT_NAME} from your phone through a bot you made. Anyone else who messages the bot gets a pairing code and nothing more, until you enter that code here.`;

const TOKEN_HINT: Record<ChannelKind, string> = {
  telegram: "Paste the token BotFather gave you.",
  discord: "Paste the token from the Bot page of your Discord application.",
};

/** Connectors → Phone and chat apps. Hidden in the web app. */
export function Channels() {
  const [shown, setShown] = useState<Shape | null>(null);
  const [tokens, setTokens] = useState<Record<ChannelKind, string>>({ telegram: "", discord: "" });
  const [code, setCode] = useState("");
  const [busy, setBusy] = useState(false);
  const { note, ok, fail } = useNote();

  useEffect(() => {
    getChannels().then(setShown, () => setShown(null));
  }, []);

  if (!shown) return null;

  async function act(step: () => Promise<Shape | null>): Promise<boolean> {
    setBusy(true);
    try {
      const next = await step();
      if (!next) return false;
      setShown(next);
      ok(next.status);
      return true;
    } catch (err) {
      fail(err);
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function connect(event: FormEvent, kind: ChannelKind) {
    event.preventDefault();
    const token = tokens[kind].trim();
    // The token leaves the page's state as soon as it is sent.
    setTokens((t) => ({ ...t, [kind]: "" }));
    if (token) await act(() => connectChannel(kind, token));
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
          <li key={ch.kind} className="card" aria-label={NAMES[ch.kind]}>
            <div className="card-title">{NAMES[ch.kind]}</div>
            <div className="card-status">
              {ch.connected ? (ch.running ? "Listening for messages" : "Connected, not listening") : "Not connected"}
            </div>
            {ch.problem && <div className="card-health">{ch.problem}</div>}
            {ch.connected ? (
              <div className="card-buttons">
                <button type="button" className="quiet danger" disabled={busy} onClick={() => void act(() => disconnectChannel(ch.kind))}>
                  Disconnect
                </button>
              </div>
            ) : (
              <form className="card-buttons" onSubmit={(e) => void connect(e, ch.kind)}>
                <label>
                  {NAMES[ch.kind]} bot token
                  <input
                    type="password"
                    autoComplete="off"
                    spellCheck={false}
                    value={tokens[ch.kind]}
                    placeholder={TOKEN_HINT[ch.kind]}
                    onChange={(e) => setTokens((t) => ({ ...t, [ch.kind]: e.target.value }))}
                  />
                </label>
                <button type="submit" disabled={busy || !tokens[ch.kind].trim()}>
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
              {p.label || "Someone"} on {NAMES[p.channel]} was sent a code
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
                {NAMES[p.channel]} {p.sender}
              </span>
              <button
                type="button"
                className="quiet"
                disabled={busy}
                aria-label={`Unpair ${p.sender} on ${NAMES[p.channel]}`}
                onClick={() => void act(() => unpairChannelSender(p.channel, p.sender))}
              >
                Unpair
              </button>
            </li>
          ))}
        </ul>
      )}
      <StatusLine note={note} />
    </section>
  );
}
