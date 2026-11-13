// Hangout tab strip under the title bar: Switch, Pin, Archive, Rename, Delete
// (soft), Clear (asks first), and restore menus for Archived / Recently deleted.
// Main sits first and only offers Clear: it can't be archived, deleted,
// renamed or unpinned.
import { useEffect, useEffectEvent, useRef, useState } from "react";
import {
  chatTabArchive,
  chatTabPin,
  chatTabRename,
  chatTabRestoreDeleted,
  chatTabSoftDelete,
  chatTabSwitch,
  chatTabUnarchive,
  listArchivedChats,
  listChatTabs,
  listDeletedChats,
  type ChatTab,
} from "./api";
import { ClearChatDialog } from "./ClearChat";
import { useDismiss } from "./ui";

export const CHAT_TABS_LABEL = "Chat tabs";
export const NEW_CHAT_TAB = "New chat";
export const MAIN_TAB_HINT = "Main: always here. Clear empties it.";

function shortTitle(title: string): string {
  const words = title.trim().split(/\s+/).filter(Boolean).slice(0, 4);
  const joined = words.join(" ");
  if (joined.length <= 28) return joined || NEW_CHAT_TAB;
  return `${joined.slice(0, 27).trimEnd()}…`;
}

type Menu =
  | { kind: "tab"; id: string; at: { x: number; y: number } }
  | { kind: "tools"; at: { x: number; y: number } };

export function ChatTabs({
  activeId,
  draftEmpty,
  onSwitch,
  onNew,
  onClear,
  onError,
}: {
  activeId: string;
  /** Open empty draft shows as "New chat" until the first message names it. */
  draftEmpty: boolean;
  onSwitch: (id: string) => void;
  onNew: () => void;
  /** Clear chat `id` — parent runs the confirmed wipe. `main` when it is
   * Main, which is emptied in place rather than closed. */
  onClear: (id: string) => Promise<void>;
  onError: (note: string) => void;
}) {
  const [tabs, setTabs] = useState<ChatTab[]>([]);
  const [archived, setArchived] = useState<ChatTab[]>([]);
  const [deleted, setDeleted] = useState<ChatTab[]>([]);
  const [menu, setMenu] = useState<Menu | null>(null);
  const [askClear, setAskClear] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<{ id: string; title: string } | null>(null);
  const tick = useRef(0);

  function refresh() {
    const n = ++tick.current;
    Promise.all([listChatTabs(), listArchivedChats(), listDeletedChats()]).then(
      ([live, arch, trash]) => {
        if (n !== tick.current) return;
        setTabs(live);
        setArchived(arch);
        setDeleted(trash);
      },
      (err) => {
        if (n === tick.current) onError(String(err));
      },
    );
  }

  // Only a new active chat reloads; refresh itself is not a dependency.
  const load = useEffectEvent(() => refresh());

  useEffect(() => {
    load();
  }, [activeId]);

  async function switchTo(id: string) {
    if (id === activeId) return;
    try {
      await chatTabSwitch(id);
      onSwitch(id);
      await refresh();
    } catch (err) {
      onError(String(err));
    }
  }

  async function pin(id: string) {
    try {
      await chatTabPin(id);
      await refresh();
    } catch (err) {
      onError(String(err));
    }
  }

  async function archive(id: string) {
    try {
      await chatTabArchive(id);
      if (id === activeId) onNew();
      await refresh();
    } catch (err) {
      onError(String(err));
    }
  }

  async function softDelete(id: string) {
    try {
      await chatTabSoftDelete(id);
      if (id === activeId) onNew();
      await refresh();
    } catch (err) {
      onError(String(err));
    }
  }

  async function restoreArchived(id: string) {
    try {
      await chatTabUnarchive(id);
      onSwitch(id);
      await refresh();
    } catch (err) {
      onError(String(err));
    }
  }

  async function restoreDeleted(id: string) {
    try {
      await chatTabRestoreDeleted(id);
      onSwitch(id);
      await refresh();
    } catch (err) {
      onError(String(err));
    }
  }

  async function commitRename() {
    if (!renaming) return;
    const title = renaming.title.trim();
    const id = renaming.id;
    setRenaming(null);
    if (!title) return;
    try {
      await chatTabRename(id, title);
      await refresh();
    } catch (err) {
      onError(String(err));
    }
  }

  const shown = tabs.length > 0 ? tabs : [];
  const activeInList = shown.some((t) => t.id === activeId);
  const isMain = (id: string) => shown.some((t) => t.id === id && t.main);

  async function clear(id: string) {
    try {
      await onClear(id);
    } catch (err) {
      onError(String(err));
    }
    refresh();
  }

  return (
    <div className="chat-tabs" role="tablist" aria-label={CHAT_TABS_LABEL}>
      <div className="chat-tabs-row">
        {shown.map((tab) => {
          // Main keeps its name even while empty.
          const label =
            tab.id === activeId && draftEmpty && !tab.main ? NEW_CHAT_TAB : shortTitle(tab.title);
          const kind = tab.main ? "chat-tab main" : tab.pinned ? "chat-tab pinned" : "chat-tab";
          return (
            <button
              key={tab.id}
              type="button"
              role="tab"
              className={kind}
              aria-selected={tab.id === activeId}
              title={tab.main ? MAIN_TAB_HINT : tab.title}
              onClick={() => void switchTo(tab.id)}
              onContextMenu={(e) => {
                e.preventDefault();
                setMenu({ kind: "tab", id: tab.id, at: { x: e.clientX, y: e.clientY } });
              }}
            >
              {tab.main ? (
                <svg className="chat-tab-main-mark" viewBox="0 0 12 12" aria-hidden="true">
                  <path d="M1.5 6 6 2l4.5 4M3 5v5h6V5" fill="none" stroke="currentColor" strokeWidth="1.4" />
                </svg>
              ) : tab.pinned ? (
                <span className="chat-tab-pin" aria-hidden="true">*</span>
              ) : null}
              {label}
            </button>
          );
        })}
        {!activeInList && (
          <button
            type="button"
            role="tab"
            className="chat-tab"
            aria-selected
            aria-label={NEW_CHAT_TAB}
            title={NEW_CHAT_TAB}
          >
            {NEW_CHAT_TAB}
          </button>
        )}
        <button type="button" className="chat-tab-plus" title="New chat" aria-label="Start new chat" onClick={onNew}>
          +
        </button>
        <button
          type="button"
          className="chat-tab-tools"
          title="Chat tools"
          aria-label="Chat tools"
          onClick={(e) => setMenu({ kind: "tools", at: { x: e.clientX, y: e.clientY } })}
        >
          ...
        </button>
      </div>

      {menu?.kind === "tab" && (
        <TabContextMenu
          at={menu.at}
          onClose={() => setMenu(null)}
          onPin={() => void pin(menu.id)}
          onArchive={() => void archive(menu.id)}
          onRename={() => {
            const tab = shown.find((t) => t.id === menu.id);
            setRenaming({ id: menu.id, title: tab?.title ?? "" });
            setMenu(null);
          }}
          onDelete={() => void softDelete(menu.id)}
          onClear={() => {
            setMenu(null);
            setAskClear(menu.id);
          }}
          clearOnly={
            isMain(menu.id) ||
            (menu.id === activeId && shown.length <= 1 && draftEmpty)
          }
        />
      )}

      {menu?.kind === "tools" && (
        <ToolsMenu
          at={menu.at}
          archived={archived}
          deleted={deleted}
          onClose={() => setMenu(null)}
          onRestoreArchived={(id) => void restoreArchived(id)}
          onRestoreDeleted={(id) => void restoreDeleted(id)}
        />
      )}

      {renaming && (
        <form
          className="chat-tab-rename"
          onSubmit={(e) => {
            e.preventDefault();
            void commitRename();
          }}
        >
          <label>
            Rename chat
            <input
              autoFocus
              aria-label="Rename chat"
              value={renaming.title}
              onChange={(e) => setRenaming({ ...renaming, title: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === "Escape") setRenaming(null);
              }}
            />
          </label>
          <button type="submit">Save</button>
          <button type="button" className="quiet" onClick={() => setRenaming(null)}>
            Cancel
          </button>
        </form>
      )}

      {askClear && (
        <ClearChatDialog
          onConfirm={() => {
            const id = askClear;
            setAskClear(null);
            void clear(id);
          }}
          onClose={() => setAskClear(null)}
        />
      )}
    </div>
  );
}

function TabContextMenu({
  at,
  onClose,
  onPin,
  onArchive,
  onRename,
  onDelete,
  onClear,
  clearOnly,
}: {
  at: { x: number; y: number };
  onClose: () => void;
  onPin: () => void;
  onArchive: () => void;
  onRename: () => void;
  onDelete: () => void;
  onClear: () => void;
  clearOnly: boolean;
}) {
  const ref = useDismiss<HTMLDivElement>(true, onClose);
  return (
    <div ref={ref} className="feed-menu tab-menu" role="menu" aria-label="Chat tab" style={{ left: at.x, top: at.y }}>
      {clearOnly ? (
        <button
          type="button"
          role="menuitem"
          onClick={() => {
            onClose();
            onClear();
          }}
        >
          Clear chat…
        </button>
      ) : (
        <>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              onClose();
              onPin();
            }}
          >
            Pin
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              onClose();
              onArchive();
            }}
          >
            Archive
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              onClose();
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
              onClose();
              onDelete();
            }}
          >
            Delete
          </button>
          <button
            type="button"
            role="menuitem"
            onClick={() => {
              onClose();
              onClear();
            }}
          >
            Clear chat…
          </button>
        </>
      )}
    </div>
  );
}

function ToolsMenu({
  at,
  archived,
  deleted,
  onClose,
  onRestoreArchived,
  onRestoreDeleted,
}: {
  at: { x: number; y: number };
  archived: ChatTab[];
  deleted: ChatTab[];
  onClose: () => void;
  onRestoreArchived: (id: string) => void;
  onRestoreDeleted: (id: string) => void;
}) {
  const ref = useDismiss<HTMLDivElement>(true, onClose);
  return (
    <div ref={ref} className="feed-menu tab-menu" role="menu" aria-label="Chat tools" style={{ left: at.x, top: at.y }}>
      <p className="feed-menu-date">Archived</p>
      {archived.length === 0 ? (
        <p className="feed-menu-date">No archived chats</p>
      ) : (
        archived.map((tab) => (
          <button
            key={tab.id}
            type="button"
            role="menuitem"
            onClick={() => {
              onClose();
              onRestoreArchived(tab.id);
            }}
          >
            Restore {shortTitle(tab.title)}
          </button>
        ))
      )}
      <p className="feed-menu-date">Recently deleted</p>
      {deleted.length === 0 ? (
        <p className="feed-menu-date">Trash is empty</p>
      ) : (
        deleted.map((tab) => (
          <button
            key={tab.id}
            type="button"
            role="menuitem"
            onClick={() => {
              onClose();
              onRestoreDeleted(tab.id);
            }}
          >
            Restore {shortTitle(tab.title)}
          </button>
        ))
      )}
    </div>
  );
}

/** Hangout header: TYPE / TALK on the shell's existing Mic path. */
export function HangoutHeader({
  mode,
  onMode,
}: {
  mode: "talk" | "type";
  onMode: (mode: "talk" | "type") => void;
}) {
  const talk = mode === "talk";
  return (
    <div className="hangout-header" aria-label="Hangout">
      <div className="hangout-copy">
        <h2>Hangout</h2>
        <p>{talk ? "Voice on · just talking" : "Here · tap Talk to speak"}</p>
      </div>
      <div className="hangout-modes" role="group" aria-label="Hangout mode">
        <button type="button" aria-pressed={talk} className={talk ? "hangout-mode on" : "hangout-mode"} onClick={() => onMode("talk")}>
          TALK
        </button>
        <button
          type="button"
          aria-pressed={!talk}
          className={!talk ? "hangout-mode on" : "hangout-mode"}
          onClick={() => onMode("type")}
        >
          TYPE
        </button>
      </div>
    </div>
  );
}
