import { useEffect, useState, type ReactNode } from "react";
import { companionFrame, companionSetNote, type CompanionFrame } from "./api";
import { GameWatch } from "./GameWatch";

export const NO_NEWS = "No news since you last played.";
export const NEWS_UNAVAILABLE = "Couldn't read the news for it just now.";

export type CompanionTarget =
  | { kind: "steam"; appId: number; lastPlayed: number }
  | { kind: "battlenet"; seed: number };

/** Note key used for load_notes / save_notes. */
export function noteIdFor(target: CompanionTarget): number {
  return target.kind === "steam" ? target.appId : target.seed;
}

/**
 * Per-game companion frame: notes, news since last played, guides link, and
 * a slot for the per-game screens (cards 712 / 713).
 */
export function GameCompanion({
  target,
  children,
}: {
  target: CompanionTarget;
  children?: ReactNode;
}) {
  const [frame, setFrame] = useState<CompanionFrame | null>(null);
  const [draft, setDraft] = useState("");
  const [status, setStatus] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const noteId = noteIdFor(target);
  const steamAppId = target.kind === "steam" ? target.appId : null;
  const lastPlayed = target.kind === "steam" ? target.lastPlayed : 0;

  // Parent remounts with `key` when the game changes, so frame/draft start
  // empty without a synchronous setState in this effect.
  useEffect(() => {
    let live = true;
    companionFrame(noteId, lastPlayed, steamAppId).then(
      (f) => {
        if (!live) return;
        setFrame(f);
        setDraft(f.note);
      },
      (err) => {
        if (live) setStatus(String(err));
      },
    );
    return () => {
      live = false;
    };
  }, [noteId, lastPlayed, steamAppId]);

  async function save() {
    setBusy(true);
    try {
      const saved = await companionSetNote(noteId, draft);
      setDraft(saved);
      setFrame((f) => (f ? { ...f, note: saved } : f));
      setStatus(saved ? "Note saved." : "Note cleared.");
    } catch (err) {
      setStatus(String(err));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="game-companion" role="region" aria-label="Game companion">
      {status && (
        <p className="note" role="status">
          {status}
        </p>
      )}
      <label className="game-companion-note">
        <span>Your note</span>
        <textarea
          aria-label="Game note"
          rows={3}
          value={draft}
          maxLength={500}
          onChange={(e) => setDraft(e.target.value)}
          placeholder="Leave a note for next time"
        />
      </label>
      <button type="button" disabled={busy || frame === null} onClick={() => void save()}>
        Save note
      </button>
      <div className="game-companion-news" aria-label="News since last played">
        <h3>Since you last played</h3>
        {frame === null && !status && <p className="card-status">Loading…</p>}
        {frame && frame.news.length === 0 && (
          <p className="card-status">{target.kind === "steam" ? NO_NEWS : NEWS_UNAVAILABLE}</p>
        )}
        {frame && frame.news.length > 0 && (
          <ul>
            {frame.news.map((n) => (
              <li key={n.url}>
                <a href={n.url} target="_blank" rel="noreferrer">
                  {n.title}
                </a>
              </li>
            ))}
          </ul>
        )}
      </div>
      {frame?.guidesUrl && (
        <p className="game-companion-guides">
          <a href={frame.guidesUrl} target="_blank" rel="noreferrer">
            Top-rated community guides
          </a>
        </p>
      )}
      <GameWatch compact />
      <div className="game-companion-slot" aria-label="Game screen">
        {children ?? <p className="card-status">Game details land here in a later update.</p>}
      </div>
    </div>
  );
}
