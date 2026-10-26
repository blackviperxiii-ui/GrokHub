import { useEffect, useState, type FormEvent } from "react";
import {
  addInterest,
  getInterests,
  interestAct,
  type InterestAct,
  type InterestPage,
  type InterestTopic,
} from "./api";

export const INTERESTS_NOTE =
  "What Pulse looks for. It starts from your memory, recent chats and liked posts, then learns from what you open, like and skip. Pinned topics always come first; muted ones never show up.";
export const INTERESTS_EMPTY =
  "No topics yet. Add a few, or let the first feed lookup pick some from your memory and chats.";

function line(t: InterestTopic) {
  if (t.muted) return "Muted";
  return t.pinned ? `Pinned · ${t.source}` : t.source;
}

/** Settings → Pulse: the interest profile, from the same file as the cabin. */
export function Interests() {
  const [page, setPage] = useState<InterestPage | null>(null);
  const [draft, setDraft] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getInterests().then(setPage, (err) => setNote(String(err)));
  }, []);

  async function run(work: () => Promise<InterestPage>) {
    setBusy(true);
    try {
      const next = await work();
      setPage(next);
      setNote(next.note || null);
      return true;
    } catch (err) {
      setNote(String(err));
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function add(event: FormEvent) {
    event.preventDefault();
    if (!draft.trim() || busy) return;
    if (await run(() => addInterest(draft))) setDraft("");
  }

  const act = (t: InterestTopic, what: InterestAct) => void run(() => interestAct(t.topic, what));

  return (
    <div className="interests" role="group" aria-label="Interests">
      <h2>Interests</h2>
      <p className="note">{INTERESTS_NOTE}</p>
      <form className="labs-row" onSubmit={(e) => void add(e)}>
        <label>
          Add a topic
          <input
            value={draft}
            maxLength={40}
            placeholder="air compressors"
            onChange={(e) => setDraft(e.target.value)}
          />
        </label>
        <button type="submit" disabled={busy || !draft.trim()}>
          Add
        </button>
      </form>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      {page === null && !note && <p className="empty">Loading interests…</p>}
      {page && page.topics.length === 0 && <p className="empty">{INTERESTS_EMPTY}</p>}
      {page && page.topics.length > 0 && (
        <ul className="rows" aria-label="Topics">
          {page.topics.map((t) => (
            <li key={t.topic} className={t.muted ? "card muted" : "card"} aria-label={t.topic}>
              <div className="card-title">{t.topic}</div>
              <div className="card-status">{line(t)}</div>
              <div className="card-buttons">
                <button type="button" className="quiet" disabled={busy} onClick={() => act(t, { kind: "pin", on: !t.pinned })}>
                  {t.pinned ? "Unpin" : "Pin"}
                </button>
                <button type="button" className="quiet" disabled={busy} onClick={() => act(t, { kind: "mute", on: !t.muted })}>
                  {t.muted ? "Unmute" : "Mute"}
                </button>
                <button type="button" className="quiet" disabled={busy} onClick={() => act(t, { kind: "remove" })}>
                  Remove
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
