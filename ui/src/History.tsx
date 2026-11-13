import {
  useEffect,
  useEffectEvent,
  useRef,
  useState,
  type FormEvent,
} from "react";
import {
  cancelDeleteAll,
  deleteAllChats,
  deleteChat,
  exportChat,
  listChats,
  listProjectNodes,
  listReadOnlyChats,
  pinChat,
  renameChat,
  searchChats,
  searchSteps,
  viewReadOnlyChat,
  type ChatRow,
  type DeleteAllResult,
  type ProjectNode,
  type ReadOnlyRow,
  type SavedFile,
  type StepRow,
  type Transcript,
} from "./api";
import { SavedCard } from "./SavedCard";
import { ConfirmRow, StatusLine, useDismiss, useNote } from "./ui";

/** A search waits this long after the last key, like the cabin's History. */
export const SEARCH_DELAY_MS = 250;

export const HISTORY_EMPTY =
  "No saved chats yet. Chats you start here show up on this page.";
export const HISTORY_NO_HITS = "No chats match that.";

/** Delete all's window: a second click inside it deletes. Like the cabin's. */
export const DELETE_ALL_ARM_MS = 5000;

/** Delete all, pressed with a search typed and no saved chat anywhere. */
export const DELETE_ALL_NONE = "There are no saved chats to delete.";

/** Delete all's answer as the app sends it: with the click that deletes,
 * `ids` names the chats that went, and `error` says why it stopped when a
 * chat could not be deleted (the chats before that one are gone). */
type DeleteAllAnswer = DeleteAllResult & { ids?: string[]; error?: string };

/** A project or folder with its children, in the file's order. */
export interface TreeNode {
  node: ProjectNode;
  children: TreeNode[];
}

/** The sidebar list as a tree. A row whose parent isn't there sits at the top. */
export function projectTree(nodes: ProjectNode[]): TreeNode[] {
  const ids = new Set(nodes.map((n) => n.id));
  const build = (parent: string | null): TreeNode[] =>
    nodes
      .filter((n) =>
        parent === null ? !n.parent || !ids.has(n.parent) : n.parent === parent,
      )
      .map((node) => ({
        node,
        children: node.kind === "folder" ? build(node.id) : [],
      }));
  return build(null);
}

export type Group = "Today" | "This week" | "Older";

const DAY_MS = 24 * 60 * 60 * 1000;

/** Today is since local midnight; This week is the six days before it. */
export function groupOf(updated: number, now: Date = new Date()): Group {
  const midnight = new Date(
    now.getFullYear(),
    now.getMonth(),
    now.getDate(),
  ).getTime();
  if (updated >= midnight) return "Today";
  if (updated >= midnight - 6 * DAY_MS) return "This week";
  return "Older";
}

/** Rows in their groups, in page order, leaving out empty groups. */
export function grouped(
  rows: ChatRow[],
  now: Date = new Date(),
): [Group, ChatRow[]][] {
  const order: Group[] = ["Today", "This week", "Older"];
  return order
    .map((group): [Group, ChatRow[]] => [
      group,
      rows.filter((row) => groupOf(row.updated, now) === group),
    ])
    .filter(([, list]) => list.length > 0);
}

/** "3:04 PM" today, "Mon 3:04 PM" this week, "Oct 26, 2026" before that. */
export function whenLabel(updated: number, now: Date = new Date()): string {
  const at = new Date(updated);
  const group = groupOf(updated, now);
  if (group === "Today")
    return at.toLocaleTimeString(undefined, {
      hour: "numeric",
      minute: "2-digit",
    });
  if (group === "This week") {
    return at.toLocaleString(undefined, {
      weekday: "short",
      hour: "numeric",
      minute: "2-digit",
    });
  }
  return at.toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
  });
}

function ChatMenu({
  title,
  pinned,
  main,
  onPin,
  onRename,
  onExport,
  onDelete,
}: {
  title: string;
  pinned: boolean;
  /** Main stays put: only Export. */
  main: boolean;
  onPin: () => void;
  onRename: () => void;
  onExport: () => void;
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
          {!main && (
            <>
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  setMenu(false);
                  onPin();
                }}
              >
                {pinned ? "Unpin" : "Pin"}
              </button>
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
            </>
          )}
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              setMenu(false);
              onExport();
            }}
          >
            Export
          </button>
          {!main && (
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
          )}
        </div>
      )}
    </div>
  );
}

/** The shell's saved chats with search. Pinned chats list first, then the
 * Projects tree (each project's chats), then the rest by day. Grok CLI and
 * cabin chats list read only below. A click opens a chat in Chat; the menu
 * pins, renames, exports or deletes it (Delete asks first). Delete all asks
 * twice: the first click arms it for a few seconds and shows how many chats
 * the app will delete, which is every chat, whatever a search lists.
 * `onDeleted` is called once for each chat that was deleted. */
export function History({
  onOpen,
  onDeleted,
}: {
  onOpen: (id: string) => void;
  onDeleted: (id: string) => void;
}) {
  const [rows, setRows] = useState<ChatRow[] | null>(null);
  const [nodes, setNodes] = useState<ProjectNode[]>([]);
  const [readOnly, setReadOnly] = useState<ReadOnlyRow[]>([]);
  const [steps, setSteps] = useState<StepRow[]>([]);
  const [query, setQuery] = useState("");
  const [renaming, setRenaming] = useState<{
    id: string;
    title: string;
  } | null>(null);
  const [asking, setAsking] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [saved, setSaved] = useState<SavedFile | null>(null);
  // Folders and projects the person folded or unfolded here; the file's own
  // open flag decides the rest.
  const [folded, setFolded] = useState<Record<string, boolean>>({});
  const [viewing, setViewing] = useState<Transcript | null>(null);
  // The number of chats the app said a second click deletes; null when
  // Delete all is not armed.
  const [armedCount, setArmedCount] = useState<number | null>(null);
  const armed = armedCount !== null;
  const { note, ok, fail, clear } = useNote();
  // Only the newest search may fill the page.
  const asked = useRef(0);
  const armTimer = useRef<number | undefined>(undefined);

  function refresh(q: string) {
    const n = ++asked.current;
    const text = q.trim();
    const current = (fn: () => void) => {
      if (n === asked.current) fn();
    };
    (text ? searchChats(text) : listChats()).then(
      (found) => current(() => setRows(found)),
      (err) => current(() => fail(err)),
    );
    // The extras never block the chat list: a failure there leaves them empty.
    listProjectNodes().then(
      (found) => current(() => setNodes(found)),
      () => current(() => setNodes([])),
    );
    listReadOnlyChats(text).then(
      (found) => current(() => setReadOnly(found)),
      () => current(() => setReadOnly([])),
    );
    (text ? searchSteps(text) : Promise.resolve([] as StepRow[])).then(
      (found) => current(() => setSteps(found)),
      () => current(() => setSteps([])),
    );
  }

  // Only a new query searches; refresh itself is not a dependency.
  const search = useEffectEvent((q: string) => refresh(q));

  useEffect(() => {
    if (!query.trim()) {
      search("");
      return;
    }
    const timer = window.setTimeout(() => search(query), SEARCH_DELAY_MS);
    return () => window.clearTimeout(timer);
  }, [query]);

  // Leaving the page (it stays mounted, hidden) drops an armed Delete all:
  // its window lapses in the app while nobody is looking at the button.
  const leave = useEffectEvent(() => {
    if (armed) disarm();
  });

  useEffect(() => () => leave(), []);

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

  async function saveCopy(id: string) {
    clear();
    setSaved(null);
    try {
      setSaved(await exportChat(id));
    } catch (err) {
      fail(err);
    }
  }

  function remove(id: string) {
    setAsking(null);
    void act(async () => {
      await deleteChat(id);
      onDeleted(id);
    });
  }

  function disarm() {
    window.clearTimeout(armTimer.current);
    setArmedCount(null);
    void cancelDeleteAll().catch(() => {});
  }

  /** The first click arms; the second (inside the window) deletes. The app
   * keeps the clock, so a double-click's second half only keeps it armed.
   * The app also keeps the list: it counts the chats on the first click and
   * deletes those on the second, so the button and Chat go by its answer,
   * never by the rows a search left on screen. */
  function deleteAll() {
    // Not `disabled` while busy: that would drop keyboard focus to the page.
    if (busy) return;
    let none = false;
    let stopped: string | undefined;
    void act(async () => {
      const result: DeleteAllAnswer = await deleteAllChats();
      if (result.state === "deleted") {
        window.clearTimeout(armTimer.current);
        setArmedCount(null);
        for (const id of result.ids ?? []) onDeleted(id);
        stopped = result.error;
        return;
      }
      if (result.count === 0) {
        none = true;
        disarm();
        return;
      }
      if (!armed) {
        armTimer.current = window.setTimeout(disarm, DELETE_ALL_ARM_MS);
      }
      setArmedCount(result.count);
    }).then(() => {
      if (none) ok(DELETE_ALL_NONE);
      if (stopped) fail(stopped);
    });
  }

  async function view(id: string) {
    if (viewing?.id === id) {
      setViewing(null);
      return;
    }
    try {
      setViewing(await viewReadOnlyChat(id));
      clear();
    } catch (err) {
      fail(err);
    }
  }

  const renderRow = (row: ChatRow) => (
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
          <button
            type="button"
            className="quiet"
            onClick={() => setRenaming(null)}
          >
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
            <span className="card-status">
              {row.pinned ? "Pinned · " : ""}
              {whenLabel(row.updated)}
            </span>
          </button>
          <ChatMenu
            title={row.title}
            pinned={!!row.pinned}
            main={!!row.main}
            onPin={() => void act(async () => void (await pinChat(row.id)))}
            onRename={() => setRenaming({ id: row.id, title: row.title })}
            onExport={() => void saveCopy(row.id)}
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
  );

  const all = rows ?? [];
  const pinned = all.filter((r) => r.pinned);
  const knownProjects = new Set(
    nodes.filter((n) => n.kind === "project").map((n) => n.id),
  );
  const inProject = (r: ChatRow) => !!r.project && knownProjects.has(r.project);
  const projectChats = (id: string) =>
    all.filter((r) => !r.pinned && r.project === id);
  const rest = all.filter((r) => !r.pinned && !inProject(r));
  const searching = query.trim() !== "";

  const hasChats = (t: TreeNode): boolean =>
    t.node.kind === "project"
      ? projectChats(t.node.id).length > 0
      : t.children.some(hasChats);

  const renderNode = (t: TreeNode) => {
    // A search shows only the branches that hold a hit.
    if (searching && !hasChats(t)) return null;
    const open = folded[t.node.id] ?? t.node.open;
    const chats = t.node.kind === "project" ? projectChats(t.node.id) : [];
    return (
      <li key={t.node.id} className="project-node">
        <button
          type="button"
          className="quiet project-toggle"
          aria-expanded={open}
          onClick={() => setFolded((f) => ({ ...f, [t.node.id]: !open }))}
        >
          {open ? "▾" : "▸"} {t.node.name}
          {t.node.kind === "project" ? ` (${chats.length})` : ""}
        </button>
        {open && (
          <ul className="rows project-children">
            {chats.map(renderRow)}
            {t.node.kind === "project" && chats.length === 0 && (
              <li className="empty">No chats yet.</li>
            )}
            {t.children.map(renderNode)}
          </ul>
        )}
      </li>
    );
  };

  const tree = projectTree(nodes);
  const sourceLabel = (r: ReadOnlyRow) =>
    r.source === "cli" ? "Grok CLI" : "Cabin";

  return (
    <section className="history" aria-label="History">
      <div className="history-head">
        <input
          type="search"
          aria-label="Search chats"
          placeholder="Search chats"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
        />
        <div className="card-buttons">
          <button
            type="button"
            className="danger"
            disabled={all.length === 0 && !searching && !armed}
            aria-disabled={busy}
            onClick={deleteAll}
            // A held Enter clicks again on every key repeat, and a repeat
            // half a second in would be the click that deletes.
            onKeyDown={(event) => {
              if (event.repeat && event.key === "Enter") event.preventDefault();
            }}
          >
            {armed
              ? `Delete all ${armedCount} ${armedCount === 1 ? "chat" : "chats"}?`
              : "Delete all"}
          </button>
          {armed && (
            <button type="button" className="quiet" onClick={disarm}>
              Cancel
            </button>
          )}
        </div>
      </div>
      {armed && (
        <p className="history-note" role="alert">
          Click again to delete every chat and its saved files
          {searching ? ", not only the chats this search lists" : ""}. This
          can't be undone.
        </p>
      )}
      <StatusLine note={note} />
      {saved && <SavedCard saved={saved} onClose={() => setSaved(null)} />}
      {rows === null && !note && <p className="empty">Loading chats…</p>}
      {rows?.length === 0 && readOnly.length === 0 && steps.length === 0 && (
        <p className="empty">{searching ? HISTORY_NO_HITS : HISTORY_EMPTY}</p>
      )}
      {pinned.length > 0 && (
        <section className="history-group" aria-label="Pinned">
          <h3>Pinned</h3>
          <ul className="rows">{pinned.map(renderRow)}</ul>
        </section>
      )}
      {tree.length > 0 && tree.some((t) => !searching || hasChats(t)) && (
        <section className="history-group" aria-label="Projects">
          <h3>Projects</h3>
          <ul className="rows project-tree">{tree.map(renderNode)}</ul>
        </section>
      )}
      {grouped(rest).map(([group, list]) => (
        <section key={group} className="history-group" aria-label={group}>
          <h3>{group}</h3>
          <ul className="rows">{list.map(renderRow)}</ul>
        </section>
      ))}
      {steps.length > 0 && (
        <section className="history-group" aria-label="Tool steps">
          <h3>Tool steps</h3>
          <ul className="rows">
            {steps.map((step) => (
              <li
                key={`${step.chat}:${step.turn}:${step.line}`}
                className="card chat-row"
                aria-label={step.line}
              >
                <button
                  type="button"
                  className="chat-open"
                  onClick={() => onOpen(step.chat)}
                >
                  <span className="card-title">{step.line}</span>
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
      {readOnly.length > 0 && (
        <section className="history-group" aria-label="Read only chats">
          <h3>Grok CLI and cabin chats</h3>
          <p className="empty">
            Read only: they open here to look at, and are never changed.
          </p>
          <ul className="rows">
            {readOnly.map((r) => (
              <li key={r.id} className="card chat-row" aria-label={r.title}>
                <div className="chat-row-head">
                  <button
                    type="button"
                    className="chat-open"
                    aria-expanded={viewing?.id === r.id}
                    title={`View ${r.title}`}
                    onClick={() => void view(r.id)}
                  >
                    <span className="card-title">{r.title}</span>
                    <span className="card-status">
                      {sourceLabel(r)} · Read only
                      {r.updated > 0 ? ` · ${whenLabel(r.updated)}` : ""}
                    </span>
                  </button>
                </div>
                {viewing?.id === r.id && (
                  <div
                    className="readonly-view"
                    role="region"
                    aria-label={`${r.title} (read only)`}
                  >
                    {viewing.earlier > 0 && (
                      <p className="empty">
                        {viewing.earlier} earlier lines not shown.
                      </p>
                    )}
                    {viewing.lines.length === 0 && (
                      <p className="empty">Nothing to show.</p>
                    )}
                    {viewing.lines.map((line, n) => (
                      <p
                        key={n}
                        className={`readonly-line readonly-${line.role}`}
                      >
                        <strong>
                          {line.role === "user"
                            ? "You"
                            : line.role === "tools"
                              ? "Tools"
                              : "Reply"}
                          :{" "}
                        </strong>
                        {line.text}
                      </p>
                    ))}
                  </div>
                )}
              </li>
            ))}
          </ul>
        </section>
      )}
    </section>
  );
}
