import { useEffect, useRef, useState, type FormEvent } from "react";
import { deleteChat, listChats, renameChat, searchChats, type ChatRow } from "./api";
import { ConfirmRow, StatusLine, useDismiss, useNote } from "./ui";

/** A search waits this long after the last key, like the cabin's History. */
export const SEARCH_DELAY_MS = 250;

export const HISTORY_EMPTY = "No saved chats yet. Chats you start here show up on this page.";
export const HISTORY_NO_HITS = "No chats match that.";

export type Group = "Today" | "This week" | "Older";

const DAY_MS = 24 * 60 * 60 * 1000;

/** Today is since local midnight; This week is the six days before it. */
export function groupOf(updated: number, now: Date = new Date()): Group {
  const midnight = new Date(now.getFullYear(), now.getMonth(), now.getDate()).getTime();
  if (updated >= midnight) return "Today";
  if (updated >= midnight - 6 * DAY_MS) return "This week";
  return "Older";
}

/** Rows in their groups, in page order, leaving out empty groups. */
export function grouped(rows: ChatRow[], now: Date = new Date()): [Group, ChatRow[]][] {
  const order: Group[] = ["Today", "This week", "Older"];
  return order
    .map((group): [Group, ChatRow[]] => [group, rows.filter((row) => groupOf(row.updated, now) === group)])
    .filter(([, list]) => list.length > 0);
}

/** "3:04 PM" today, "Mon 3:04 PM" this week, "Oct 26, 2026" before that. */
export function whenLabel(updated: number, now: Date = new Date()): string {
  const at = new Date(updated);
  const group = groupOf(updated, now);
  if (group === "Today") return at.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" });
  if (group === "This week") {
    return at.toLocaleString(undefined, { weekday: "short", hour: "numeric", minute: "2-digit" });
  }
  return at.toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
}

function ChatMenu({
  title,
  onRename,
  onDelete,
}: {
  title: string;
  onRename: () => void;
  onDelete: () => void;
}) {
  const [menu, setMenu] = useState(false);
  const corner = useDismiss<HTMLDivElement>(menu, () => setMenu(false));
  return (
    <div className="feed-corner" ref={corner}>
      <button
        type="button"
        className="quiet"
        aria-label={`More for ${title}`}
        aria-haspopup="menu"
        aria-expanded={menu}
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
              onRename();
            }}
          >
            Rename
          </button>
          <button
            type="button"
            role="menuitem"
            className="danger"
            onClick={() => {
              setMenu(false);
              onDelete();
            }}
          >
            Delete
          </button>
        </div>
      )}
    </div>
  );
}

/** The shell's saved chats, newest first, with search. A click opens one in
 * Chat; the menu renames or deletes it (Delete asks first). */
export function History({
  onOpen,
  onDeleted,
}: {
  onOpen: (id: string) => void;
  onDeleted: (id: string) => void;
}) {
  const [rows, setRows] = useState<ChatRow[] | null>(null);
  const [query, setQuery] = useState("");
  const [renaming, setRenaming] = useState<{ id: string; title: string } | null>(null);
  const [asking, setAsking] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const { note, fail, clear } = useNote();
  // Only the newest search may fill the page.
  const asked = useRef(0);

  function refresh(q: string) {
    const n = ++asked.current;
    const pending = q.trim() ? searchChats(q) : listChats();
    pending.then(
      (found) => {
        if (n === asked.current) setRows(found);
      },
      (err) => {
        if (n === asked.current) fail(err);
      },
    );
  }

  useEffect(() => {
    if (!query.trim()) {
      refresh("");
      return;
    }
    const timer = window.setTimeout(() => refresh(query), SEARCH_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [query]);

  async function act(step: () => Promise<void>) {
    setBusy(true);
    try {
      await step();
      clear();
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
      refresh(query);
    }
  }

  function saveName(event: FormEvent) {
    event.preventDefault();
    if (!renaming) return;
    const { id, title } = renaming;
    setRenaming(null);
    void act(() => renameChat(id, title));
  }

  function remove(id: string) {
    setAsking(null);
    void act(async () => {
      await deleteChat(id);
      onDeleted(id);
    });
  }

  return (
    <section className="history" aria-label="History">
      <input
        type="search"
        aria-label="Search chats"
        placeholder="Search chats"
        value={query}
        onChange={(e) => setQuery(e.target.value)}
      />
      <StatusLine note={note} />
      {rows === null && !note && <p className="empty">Loading chats…</p>}
      {rows?.length === 0 && <p className="empty">{query.trim() ? HISTORY_NO_HITS : HISTORY_EMPTY}</p>}
      {rows &&
        grouped(rows).map(([group, list]) => (
          <section key={group} className="history-group" aria-label={group}>
            <h3>{group}</h3>
            <ul className="rows">
              {list.map((row) => (
                <li key={row.id} className="card chat-row" aria-label={row.title}>
                  {renaming?.id === row.id ? (
                    <form className="card-buttons" onSubmit={saveName}>
                      <input
                        aria-label="Chat name"
                        value={renaming.title}
                        autoFocus
                        onChange={(e) => setRenaming({ id: row.id, title: e.target.value })}
                      />
                      <button type="submit" disabled={!renaming.title.trim()}>
                        Save
                      </button>
                      <button type="button" className="quiet" onClick={() => setRenaming(null)}>
                        Cancel
                      </button>
                    </form>
                  ) : (
                    <div className="chat-row-head">
                      <button
                        type="button"
                        className="chat-open"
                        title={`Open ${row.title}`}
                        onClick={() => onOpen(row.id)}
                      >
                        <span className="card-title">{row.title}</span>
                        <span className="card-status">{whenLabel(row.updated)}</span>
                      </button>
                      <ChatMenu
                        title={row.title}
                        onRename={() => setRenaming({ id: row.id, title: row.title })}
                        onDelete={() => setAsking(row.id)}
                      />
                    </div>
                  )}
                  {asking === row.id && (
                    <ConfirmRow
                      question={`Delete ${row.title}?`}
                      action="Delete"
                      busy={busy}
                      onConfirm={() => remove(row.id)}
                      onCancel={() => setAsking(null)}
                    />
                  )}
                </li>
              ))}
            </ul>
          </section>
        ))}
    </section>
  );
}
