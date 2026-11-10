import { useEffect, useState } from "react";
import { pulseSaved, pulseUnsave, type SavedFind } from "./api";
import { Snippet, fullDate } from "./FeedCard";
import { ExternalLink, useDismiss } from "./ui";

export const SAVED_EMPTY = "Nothing saved yet. Save a post in the feed and it stays here.";

/** One saved card: always open, no row of buttons. Its ⋯ menu holds one
 * item, Remove from saved. */
function SavedCard({
  find,
  busy,
  onRemove,
}: {
  find: SavedFind;
  busy: boolean;
  onRemove: () => void;
}) {
  const [menu, setMenu] = useState(false);
  const corner = useDismiss<HTMLDivElement>(menu, () => setMenu(false));
  return (
    <li className="card post feed-card" aria-label={find.title}>
      <div className="feed-head">
        <div className="feed-title">
          <ExternalLink href={find.url}>{find.title}</ExternalLink>
        </div>
        <div className="feed-corner" ref={corner}>
          <span className="card-status" title={fullDate(find.savedMs)}>
            Saved
          </span>
          <button
            type="button"
            className="quiet icon-button"
            aria-label="More"
            aria-haspopup="menu"
            aria-expanded={menu}
            disabled={busy}
            onClick={() => setMenu((m) => !m)}
          >
            ⋯
          </button>
          {menu && (
            <div className="feed-menu" role="menu">
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  setMenu(false);
                  onRemove();
                }}
              >
                Remove from saved
              </button>
            </div>
          )}
        </div>
      </div>
      <p className="feed-source">
        {find.sourceName || "Saved"}
        {find.because && ` · ${find.because}`}
      </p>
      {find.summary && <Snippet text={find.summary} />}
    </li>
  );
}

/** The Saved page: cards kept with Save, stored apart from the feed so
 * cleanup, expiry, archive and delete never touch them. */
export function SavedPage({ onError }: { onError: (err: unknown) => void }) {
  const [list, setList] = useState<SavedFind[] | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    pulseSaved().then(setList, onError);
  }, [onError]);
  async function remove(url: string) {
    setBusy(true);
    try {
      setList(await pulseUnsave(url));
    } catch (err) {
      onError(err);
    } finally {
      setBusy(false);
    }
  }
  if (list === null) return null;
  if (list.length === 0) return <p className="empty">{SAVED_EMPTY}</p>;
  return (
    <ul className="rows feed" aria-label="Saved">
      {list.map((find) => (
        <SavedCard key={find.url} find={find} busy={busy} onRemove={() => void remove(find.url)} />
      ))}
    </ul>
  );
}

