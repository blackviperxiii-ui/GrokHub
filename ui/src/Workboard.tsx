import { useEffect, useRef, useState, type DragEvent, type FormEvent, type KeyboardEvent } from "react";
import {
  getWorkboard,
  workboardAct,
  workboardAdd,
  workboardWork,
  type BoardAct,
  type BoardCard,
  type BoardLane,
  type BoardPage,
} from "./api";

export const BOARD_EMPTY = "No cards yet. A chat run files one here, or add your own.";
export const BOARD_HELP =
  "Drag a card to move it, or use its Move menu. Work puts the card in the chat box for you to send. The cabin keeps this same board; if it is open too, its next save can replace a change made here.";
const DRAG_TYPE = "application/x-workboard-card";

/** The cabin's Workboards page on the same workboard.json. `onWork` puts a
 * card's first message in the chat box; nothing is sent until you send it. */
export function Workboard({ onWork }: { onWork: (prompt: string) => void }) {
  const [page, setPage] = useState<BoardPage | null>(null);
  const [title, setTitle] = useState("");
  const [detail, setDetail] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [showArchived, setShowArchived] = useState(false);
  const [dragging, setDragging] = useState<{ id: string; lane: BoardLane } | null>(null);

  useEffect(() => {
    getWorkboard().then(setPage, (err) => setNote(String(err)));
  }, []);

  async function run(work: () => Promise<BoardPage>, done?: string) {
    setBusy(true);
    try {
      setPage(await work());
      setNote(done ?? null);
      return true;
    } catch (err) {
      setNote(String(err));
      // The cabin may have changed it; show what the file holds now.
      await getWorkboard().then(setPage, () => {});
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function add(event: FormEvent) {
    event.preventDefault();
    if (!title.trim() || busy) return;
    if (await run(() => workboardAdd(title, detail), "Added to Todo.")) {
      setTitle("");
      setDetail("");
    }
  }

  function act(card: BoardCard, what: BoardAct, done?: string) {
    return run(() => workboardAct(card.id, what), done);
  }

  async function work(card: BoardCard) {
    setBusy(true);
    try {
      const started = await workboardWork(card.id);
      setPage(started.page);
      setNote(null);
      onWork(started.prompt);
    } catch (err) {
      setNote(String(err));
      await getWorkboard().then(setPage, () => {});
    } finally {
      setBusy(false);
    }
  }

  function drop(event: DragEvent, lane: BoardLane) {
    event.preventDefault();
    const from = dragging;
    setDragging(null);
    if (!from || from.lane === lane || busy) return;
    const card = page?.columns.flatMap((c) => c.cards).find((c) => c.id === from.id);
    if (card) void act(card, { kind: "move", to: lane });
  }

  const total = page ? page.columns.reduce((n, c) => n + c.cards.length, 0) : 0;
  const lanes = page ? page.columns.map((c) => ({ lane: c.lane, label: c.label })) : [];

  return (
    <section className="workboard" aria-label="Workboard">
      <form className="workboard-add" onSubmit={(e) => void add(e)}>
        <input
          aria-label="Card title"
          placeholder="Card title"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
        />
        <input
          aria-label="Card detail"
          placeholder="Detail (optional)"
          value={detail}
          onChange={(e) => setDetail(e.target.value)}
        />
        <button type="submit" disabled={busy || !title.trim()}>
          Add card
        </button>
      </form>
      <p className="card-status">{BOARD_HELP}</p>
      {note && (
        <p className="status" role="status">
          {note}
        </p>
      )}
      {page === null && !note && <p className="empty">Loading the workboard…</p>}
      {page && total === 0 && <p className="empty">{BOARD_EMPTY}</p>}
      {page && total > 0 && (
        <div className="board-columns">
          {page.columns
            // Follow up shows while it has cards, or as a drop target.
            .filter((col) => col.lane !== "followUp" || col.cards.length > 0 || dragging)
            .map((col) => (
              <section
                key={col.lane}
                className={col.lane === "followUp" ? "board-column follow-up" : "board-column"}
                aria-label={col.label}
                data-drop={dragging && dragging.lane !== col.lane ? "yes" : undefined}
                onDragOver={(e) => {
                  if (dragging && dragging.lane !== col.lane) e.preventDefault();
                }}
                onDrop={(e) => drop(e, col.lane)}
              >
                <h2>
                  {col.label}
                  {col.lane === "followUp" &&
                    col.cards.some((c) => c.fresh) &&
                    ` · ${col.cards.filter((c) => c.fresh).length} new`}
                </h2>
                {col.cards.length === 0 && <p className="card-status">—</p>}
                <ul className="rows">
                  {col.cards.map((card) => (
                    <BoardCardRow
                      key={card.id}
                      card={card}
                      lane={col.lane}
                      lanes={lanes}
                      busy={busy}
                      onAct={(what, done) => act(card, what, done)}
                      onWork={() => void work(card)}
                      onDragStart={() => setDragging({ id: card.id, lane: col.lane })}
                      onDragEnd={() => setDragging(null)}
                    />
                  ))}
                </ul>
              </section>
            ))}
        </div>
      )}
      {page && page.archived.length > 0 && (
        <div className="board-archived">
          <button
            type="button"
            className="quiet"
            aria-pressed={showArchived}
            onClick={() => setShowArchived((v) => !v)}
          >
            Archived ({page.archived.length})
          </button>
          {showArchived && (
            <ul className="rows" aria-label="Archived cards">
              {page.archived.map((card) => (
                <ArchivedRow
                  key={card.id}
                  card={card}
                  busy={busy}
                  onAct={(what, done) => act(card, what, done)}
                />
              ))}
            </ul>
          )}
        </div>
      )}
    </section>
  );
}

type OnAct = (what: BoardAct, done?: string) => Promise<boolean>;

/** Delete for good, a second step so a stray click cannot remove a card. */
function DeleteButton({ card, busy, onAct }: { card: BoardCard; busy: boolean; onAct: OnAct }) {
  const [asking, setAsking] = useState(false);
  if (!asking) {
    return (
      <button type="button" className="quiet" disabled={busy} onClick={() => setAsking(true)}>
        Delete
      </button>
    );
  }
  return (
    <span className="board-confirm" role="group" aria-label="Delete this card?">
      <span>Delete this card for good?</span>
      <button
        type="button"
        className="danger"
        disabled={busy}
        onClick={() => void onAct({ kind: "delete" }, `Deleted ${card.title}.`)}
      >
        Delete card
      </button>
      <button type="button" className="quiet" onClick={() => setAsking(false)}>
        Keep
      </button>
    </span>
  );
}

function ArchivedRow({ card, busy, onAct }: { card: BoardCard; busy: boolean; onAct: OnAct }) {
  return (
    <li className="card board-card done" aria-label={card.title}>
      <div className="card-title">{card.title}</div>
      <div className="card-buttons">
        <button
          type="button"
          disabled={busy}
          onClick={() => void onAct({ kind: "restore" }, `Restored ${card.title} to Todo.`)}
        >
          Restore
        </button>
        <DeleteButton card={card} busy={busy} onAct={onAct} />
      </div>
    </li>
  );
}

/** Move to: one menu item per other column. Arrow keys walk it, Escape
 * closes it and puts focus back on Move. */
function MoveMenu({
  lane,
  lanes,
  busy,
  onMove,
}: {
  lane: BoardLane;
  lanes: { lane: BoardLane; label: string }[];
  busy: boolean;
  onMove: (to: BoardLane) => void;
}) {
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const dests = lanes.filter((l) => l.lane !== lane);

  useEffect(() => {
    if (open) menu.current?.querySelector<HTMLButtonElement>("button")?.focus();
  }, [open]);

  function close() {
    setOpen(false);
    button.current?.focus();
  }

  function onKey(event: KeyboardEvent) {
    const items = [...(menu.current?.querySelectorAll<HTMLButtonElement>("button") ?? [])];
    const at = items.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === "Escape") {
      event.preventDefault();
      close();
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      const step = event.key === "ArrowDown" ? 1 : -1;
      items[(at + step + items.length) % items.length]?.focus();
    }
  }

  return (
    <span className="board-move">
      <button
        ref={button}
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        disabled={busy}
        onClick={() => setOpen((v) => !v)}
      >
        Move
      </button>
      {open && (
        <div ref={menu} className="board-menu" role="menu" aria-label="Move to" onKeyDown={onKey}>
          {dests.map((d) => (
            <button
              key={d.lane}
              type="button"
              role="menuitem"
              onClick={() => {
                close();
                onMove(d.lane);
              }}
            >
              {d.label}
            </button>
          ))}
        </div>
      )}
    </span>
  );
}

function BoardCardRow({
  card,
  lane,
  lanes,
  busy,
  onAct,
  onWork,
  onDragStart,
  onDragEnd,
}: {
  card: BoardCard;
  lane: BoardLane;
  lanes: { lane: BoardLane; label: string }[];
  busy: boolean;
  onAct: OnAct;
  onWork: () => void;
  onDragStart: () => void;
  onDragEnd: () => void;
}) {
  const [editing, setEditing] = useState<{ title: string; detail: string } | null>(null);
  const [notes, setNotes] = useState<string | null>(null);

  async function saveEdit(event: FormEvent) {
    event.preventDefault();
    if (!editing || !editing.title.trim()) return;
    if (await onAct({ kind: "edit", title: editing.title, detail: editing.detail }, "Saved.")) {
      setEditing(null);
    }
  }

  async function saveNotes(event: FormEvent) {
    event.preventDefault();
    if (notes === null) return;
    if (await onAct({ kind: "notes", notes }, "Notes saved. Work hands them to the chat.")) {
      setNotes(null);
    }
  }

  return (
    <li
      className={card.fresh ? "card board-card fresh" : "card board-card"}
      aria-label={card.title}
      draggable={!busy && editing === null && notes === null}
      onDragStart={(e) => {
        e.dataTransfer?.setData(DRAG_TYPE, card.id);
        if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
        onDragStart();
      }}
      onDragEnd={onDragEnd}
    >
      {editing ? (
        <form className="board-edit" onSubmit={(e) => void saveEdit(e)}>
          <input
            aria-label="Title"
            value={editing.title}
            onChange={(e) => setEditing({ ...editing, title: e.target.value })}
          />
          <input
            aria-label="Detail"
            value={editing.detail}
            onChange={(e) => setEditing({ ...editing, detail: e.target.value })}
          />
          <div className="card-buttons">
            <button type="submit" disabled={busy || !editing.title.trim()}>
              Save
            </button>
            <button type="button" className="quiet" onClick={() => setEditing(null)}>
              Cancel
            </button>
          </div>
        </form>
      ) : (
        <>
          <div className="card-title">
            {card.title}
            {card.fresh && <span className="card-status"> · new</span>}
          </div>
          {card.detail && <div className="card-status">{card.detail}</div>}
          {card.report && (
            <details className="board-report">
              <summary>Latest report</summary>
              <pre>{card.report}</pre>
            </details>
          )}
          {card.notes && notes === null && <div className="board-notes">Notes: {card.notes}</div>}
        </>
      )}
      {notes !== null && (
        <form className="board-notes-edit" onSubmit={(e) => void saveNotes(e)}>
          <textarea
            aria-label="Notes for the agent"
            rows={3}
            value={notes}
            onChange={(e) => setNotes(e.target.value)}
          />
          <div className="card-buttons">
            <button type="submit" disabled={busy}>
              Save notes
            </button>
            <button type="button" className="quiet" onClick={() => setNotes(null)}>
              Cancel
            </button>
          </div>
        </form>
      )}
      {editing === null && notes === null && (
        <div className="card-buttons">
          {lane !== "done" && (
            <button type="button" disabled={busy} onClick={onWork}>
              Work
            </button>
          )}
          <button type="button" className="quiet" disabled={busy} onClick={() => setNotes(card.notes)}>
            {card.notes ? "Edit notes" : "Add notes"}
          </button>
          <button
            type="button"
            className="quiet"
            disabled={busy}
            onClick={() => setEditing({ title: card.title, detail: card.detail })}
          >
            Edit
          </button>
          <MoveMenu
            lane={lane}
            lanes={lanes}
            busy={busy}
            onMove={(to) => void onAct({ kind: "move", to })}
          />
          <button
            type="button"
            className="quiet"
            disabled={busy}
            onClick={() => void onAct({ kind: "archive" }, `Archived ${card.title}.`)}
          >
            Archive
          </button>
          <DeleteButton card={card} busy={busy} onAct={onAct} />
        </div>
      )}
    </li>
  );
}
