import { useEffect, useState } from "react";
import { getPulse, pulseAct, type PulseAct, type PulseIdea, type PulsePage } from "./api";

export const FEED_EMPTY = "No posts yet. The cabin posts here as it notices things.";
export const IDEAS_EMPTY = "No ideas yet. I'll add one when I spot something worth doing.";

type Tab = "feed" | "ideas";

/** Snooze hides a card until the next 9 AM: later today before nine. */
export function snoozedLine(now: Date = new Date()): string {
  return now.getHours() < 9 ? "Snoozed until 9 AM." : "Snoozed until tomorrow 9 AM.";
}

/** The cabin's Pulse: Feed first, Ideas second. `onRun` puts an idea's
 * action in the chat box; nothing runs until it is sent there. */
export function Pulse({ onRun }: { onRun: (text: string) => void }) {
  const [page, setPage] = useState<PulsePage | null>(null);
  const [tab, setTab] = useState<Tab>("feed");
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getPulse().then(setPage, (err) => setNote(String(err)));
  }, []);

  async function act(id: string, what: PulseAct, done?: string) {
    setBusy(true);
    try {
      setPage(await pulseAct(id, what));
      setNote(done ?? null);
      return true;
    } catch (err) {
      setNote(String(err));
      // The card may be gone; show what is left. Wait for it, so a later
      // action can't be overwritten by this older page.
      await getPulse().then(setPage, () => {});
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function run(idea: PulseIdea) {
    if (await act(idea.id, "accept")) onRun(idea.action);
  }

  const ideaCount = page?.ideas.reduce((n, g) => n + g.ideas.length, 0) ?? 0;

  return (
    <section className="pulse" aria-label="Pulse">
      <div className="switch" role="group" aria-label="Pulse view">
        <button type="button" aria-pressed={tab === "feed"} onClick={() => setTab("feed")}>
          Feed
        </button>
        <button type="button" aria-pressed={tab === "ideas"} onClick={() => setTab("ideas")}>
          Ideas
        </button>
      </div>
      {note && (
        <p className="status" role="status">
          {note}
        </p>
      )}
      {page === null && !note && <p className="empty">Loading Pulse…</p>}
      {page && tab === "feed" && (
        <>
          {page.feed.length === 0 && <p className="empty">{FEED_EMPTY}</p>}
          <ul className="rows">
            {page.feed.map((post) => (
              <li key={post.id} className="card post" aria-label={post.title}>
                <div className="card-status">
                  {post.source} · {post.ago}
                </div>
                <div className="card-title">{post.title}</div>
                {post.summary && <div className="card-detail-line plain">{post.summary}</div>}
                {post.because && <div className="card-status">Why: {post.because}</div>}
                {post.link && <div className="card-detail-line">{post.link}</div>}
                <div className="card-buttons">
                  <button
                    type="button"
                    className="quiet"
                    aria-pressed={post.loved}
                    disabled={busy || post.loved}
                    onClick={() => void act(post.id, "love")}
                  >
                    {post.loved ? "Loved" : "Love"}
                  </button>
                  <button
                    type="button"
                    className="quiet"
                    disabled={busy}
                    onClick={() => void act(post.id, "snooze", snoozedLine())}
                  >
                    Snooze
                  </button>
                  <button
                    type="button"
                    className="quiet"
                    disabled={busy}
                    onClick={() => void act(post.id, "notThis", "Got it. Less like this.")}
                  >
                    Not this
                  </button>
                  <button
                    type="button"
                    className="quiet danger"
                    disabled={busy}
                    onClick={() => void act(post.id, "dismiss")}
                  >
                    Dismiss
                  </button>
                </div>
              </li>
            ))}
          </ul>
        </>
      )}
      {page && tab === "ideas" && (
        <>
          {ideaCount === 0 && <p className="empty">{IDEAS_EMPTY}</p>}
          {page.ideas.map((group) => (
            <div key={group.label || "top"} className="idea-group">
              {group.label && <h3>{group.label}</h3>}
              <ul className="rows">
                {group.ideas.map((idea) => (
                  <li key={idea.id} className="card post" aria-label={idea.title}>
                    <div className="card-title">{idea.title}</div>
                    {idea.summary && (
                      <div className="card-detail-line plain">{idea.summary}</div>
                    )}
                    <div className="card-status">{idea.kind}</div>
                    <div className="card-buttons">
                      <button type="button" disabled={busy} onClick={() => void run(idea)}>
                        Run
                      </button>
                      <button
                        type="button"
                        className="quiet"
                        disabled={busy}
                        onClick={() => void act(idea.id, "snooze", snoozedLine())}
                      >
                        Snooze
                      </button>
                      <button
                        type="button"
                        className="quiet"
                        disabled={busy}
                        onClick={() => void act(idea.id, "notThis", "Got it. Less like this.")}
                      >
                        Not this
                      </button>
                      <button
                        type="button"
                        className="quiet danger"
                        disabled={busy}
                        onClick={() => void act(idea.id, "dismiss")}
                      >
                        Dismiss
                      </button>
                    </div>
                  </li>
                ))}
              </ul>
            </div>
          ))}
        </>
      )}
    </section>
  );
}
