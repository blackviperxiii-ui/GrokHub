import { useEffect, useRef, useState } from "react";
import {
  getFeedPrompt,
  getPulse,
  pulseAct,
  pulseDiscuss,
  pulseWhy,
  saveFeedPrompt,
  type PulseAct,
  type PulseIdea,
  type PulsePage,
} from "./api";
import { FeedCard, SkeletonRows } from "./FeedCard";

export const FEED_EMPTY = "No posts yet. Tell me what your feed should cover with Edit feed instructions.";
export const IDEAS_EMPTY = "No ideas yet. I'll add one when I spot something worth doing.";
export const PROMPT_LABEL = "What should your feed cover?";
export const SAVED_NOTE = "Saved. A new post is on its way.";
/** Posts shown at first, and added each time the list's end scrolls into view. */
export const PAGE_SIZE = 10;
/** After a save, how often and how many times to look for the new post. */
const POLL_MS = 4000;
const POLL_TRIES = 8;

type Tab = "feed" | "ideas";

/** Snooze hides a card until the next 9 AM: later today before nine. */
export function snoozedLine(now: Date = new Date()): string {
  return now.getHours() < 9 ? "Snoozed until 9 AM." : "Snoozed until tomorrow 9 AM.";
}

/** The Feed instructions sheet: one free-text box, Cancel and Save. */
function Instructions({ onClose, onSaved }: { onClose: () => void; onSaved: () => void }) {
  const [text, setText] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    getFeedPrompt().then(
      (p) => setText(p.text),
      (err) => setError(String(err)),
    );
  }, []);

  async function save() {
    if (text === null) return;
    setSaving(true);
    try {
      await saveFeedPrompt(text);
      onSaved();
    } catch (err) {
      setError(String(err));
    } finally {
      setSaving(false);
    }
  }

  return (
    <div className="card feed-instructions" role="dialog" aria-label="Edit feed instructions">
      <label htmlFor="feed-prompt">{PROMPT_LABEL}</label>
      <textarea
        id="feed-prompt"
        rows={8}
        value={text ?? ""}
        disabled={text === null}
        onChange={(e) => setText(e.target.value)}
      />
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <div className="card-buttons">
        <button type="button" className="quiet" onClick={onClose}>
          Cancel
        </button>
        <button type="button" disabled={saving || text === null} onClick={() => void save()}>
          Save
        </button>
      </div>
    </div>
  );
}

/** The cabin's Pulse: Feed first, Ideas second. `onRun` puts text in the
 * chat box (an idea's action, or a post to discuss); nothing runs until it
 * is sent there. */
export function Pulse({ onRun }: { onRun: (text: string) => void }) {
  const [page, setPage] = useState<PulsePage | null>(null);
  const [tab, setTab] = useState<Tab>("feed");
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [editing, setEditing] = useState(false);
  const [shown, setShown] = useState(PAGE_SIZE);
  const end = useRef<HTMLLIElement>(null);
  const polls = useRef<ReturnType<typeof setTimeout>[]>([]);

  useEffect(() => {
    getPulse().then(setPage, (err) => setNote(String(err)));
    const timers = polls.current;
    return () => timers.forEach(clearTimeout);
  }, []);

  const total = page?.feed.length ?? 0;
  const more = tab === "feed" && shown < total;
  useEffect(() => {
    const el = end.current;
    if (!more || !el) return;
    if (typeof IntersectionObserver === "undefined") {
      setShown(total);
      return;
    }
    const seen = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) setShown((n) => n + PAGE_SIZE);
    });
    seen.observe(el);
    return () => seen.disconnect();
  }, [more, total, shown]);

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

  async function discuss(id: string) {
    try {
      onRun(await pulseDiscuss(id));
    } catch (err) {
      setNote(String(err));
    }
  }

  /** The writer posts in the background: look again until the new post shows. */
  function watchForPost() {
    const before = new Set(page?.feed.map((p) => p.id));
    polls.current.forEach(clearTimeout);
    polls.current = [];
    const look = (left: number) => {
      polls.current.push(
        setTimeout(() => {
          getPulse().then(
            (next) => {
              setPage(next);
              if (next.feed.some((p) => !before.has(p.id))) setNote(null);
              else if (left > 1) look(left - 1);
            },
            () => {},
          );
        }, POLL_MS),
      );
    };
    look(POLL_TRIES);
  }

  const ideaCount = page?.ideas.reduce((n, g) => n + g.ideas.length, 0) ?? 0;

  return (
    <section className="pulse" aria-label="Pulse">
      <div className="pulse-head">
        <div className="switch" role="group" aria-label="Pulse view">
          <button type="button" aria-pressed={tab === "feed"} onClick={() => setTab("feed")}>
            Feed
          </button>
          <button type="button" aria-pressed={tab === "ideas"} onClick={() => setTab("ideas")}>
            Ideas
          </button>
        </div>
        {tab === "feed" && (
          <button
            type="button"
            className="quiet icon-button"
            aria-label="Edit feed instructions"
            title="Edit feed instructions"
            aria-expanded={editing}
            onClick={() => setEditing((e) => !e)}
          >
            <svg width="18" height="18" viewBox="0 0 24 24" aria-hidden="true">
              <path
                d="M4 6h10M18 6h2M4 12h4M12 12h8M4 18h12M20 18h0"
                stroke="currentColor"
                strokeWidth="2"
                strokeLinecap="round"
                fill="none"
              />
              <circle cx="16" cy="6" r="2" stroke="currentColor" strokeWidth="2" fill="none" />
              <circle cx="10" cy="12" r="2" stroke="currentColor" strokeWidth="2" fill="none" />
              <circle cx="18" cy="18" r="2" stroke="currentColor" strokeWidth="2" fill="none" />
            </svg>
          </button>
        )}
      </div>
      {editing && tab === "feed" && (
        <Instructions
          onClose={() => setEditing(false)}
          onSaved={() => {
            setEditing(false);
            setNote(SAVED_NOTE);
            watchForPost();
          }}
        />
      )}
      {note && (
        <p className="status" role="status">
          {note}
        </p>
      )}
      {tab === "feed" && (
        <ul className="rows feed" aria-busy={page === null}>
          {page === null && !note && <SkeletonRows />}
          {page?.feed.slice(0, shown).map((post) => (
            <FeedCard
              key={post.id}
              post={post}
              busy={busy}
              onLove={() => void act(post.id, "love")}
              onDiscuss={() => void discuss(post.id)}
              onDelete={() => void act(post.id, "delete", "Deleted.")}
              onWhy={() => pulseWhy(post.id)}
            />
          ))}
          {more && (
            <li ref={end} className="card post feed-card skeleton" aria-hidden="true">
              <span className="skeleton-line wide" />
              <span className="skeleton-line short" />
            </li>
          )}
        </ul>
      )}
      {page && tab === "feed" && total === 0 && <p className="empty">{FEED_EMPTY}</p>}
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
