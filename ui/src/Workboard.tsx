import {
  useEffect,
  useRef,
  useState,
  type CSSProperties,
  type DragEvent,
  type FormEvent,
  type KeyboardEvent,
} from "react";
import {
  getWorkboard,
  workboardAct,
  workboardAdd,
  workboardLog,
  workboardResume,
  workboardStamp,
  workboardWork,
  type BoardAct,
  type BoardCard,
  type BoardLane,
  type BoardPage,
  type WorkLogLine,
} from "./api";
import { cardsByChat } from "./activity";
import { newConversationId } from "./Chat";

export const BOARD_EMPTY = "No cards yet. A chat run files one here, or add your own.";
export const BOARD_HELP =
  "Drag a card to move it, or use its Move menu. Work puts the card in the chat box for you to send. The cabin keeps this same board, and neither overwrites the other's changes.";
/** How often the page checks whether the cabin saved the board. */
export const STAMP_EVERY_MS = 3000;
/** Longest "Left off" text on a compact card, like the cabin's meta line. */
const LEFT_OFF_MAX = 60;
const DRAG_TYPE = "application/x-workboard-card";

const STEP_MARK: Record<BoardCard["steps"][number]["status"], string> = {
  done: "✓",
  doing: "→",
  planned: "·",
  skipped: "–",
  failed: "×",
};

function clip(text: string, max: number): string {
  const one = text.replace(/\s+/g, " ").trim();
  return one.length > max ? `${one.slice(0, max - 1)}…` : one;
}

/** "just now", "5m ago", "3h ago", "2d ago". */
export function ago(at: number, now: number): string {
  const s = Math.max(0, Math.round((now - at) / 1000));
  if (s < 60) return "just now";
  if (s < 3600) return `${Math.floor(s / 60)}m ago`;
  if (s < 86400) return `${Math.floor(s / 3600)}h ago`;
  return `${Math.floor(s / 86400)}d ago`;
}

type Lanes = { lane: BoardLane; label: string }[];

/** The cabin's Workboards page on the same workboard.json. Every change goes
 * through the shared board lock and merge, so neither app overwrites the
 * other, and the page reloads when the file changes. `onWork` puts a card's
 * first message in the chat box and `onResume` a paused run's; nothing is
 * sent until you send it, and nothing here reaches the model by itself. */
export function Workboard({
  onWork,
  onResume,
  resumable = true,
  onCards,
  openCard,
}: {
  onWork: (prompt: string) => void;
  onResume?: (resume: { conversationId: string; prompt: string }) => void;
  /** False in the web app, where Resume stays on your computer. */
  resumable?: boolean;
  /** The chat-to-card map, after every load. */
  onCards?: (byChat: Record<string, string>) => void;
  /** Opens this card, as the Activity dock asks. `n` repeats a request. */
  openCard?: { id: string; n: number };
}) {
  const [page, setPage] = useState<BoardPage | null>(null);
  const [title, setTitle] = useState("");
  const [detail, setDetail] = useState("");
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [showArchived, setShowArchived] = useState(false);
  const [dragging, setDragging] = useState<{ id: string; lane: BoardLane } | null>(null);
  const [open, setOpen] = useState<string | null>(null);

  // The clock for "5m ago": read when the board or a log loads, and each minute.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 60_000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    getWorkboard().then(
      (next) => {
        setPage(next);
        setNow(Date.now());
      },
      (err) => setNote(String(err)),
    );
  }, []);

  useEffect(() => {
    if (page) onCards?.(cardsByChat(page));
  }, [page, onCards]);

  // The cabin may save meanwhile: check the file's mtime, reload on a change.
  const stamp = page?.stamp;
  useEffect(() => {
    if (stamp === undefined) return;
    const timer = window.setInterval(() => {
      workboardStamp().then(
        (current) => {
          if (current !== stamp) getWorkboard().then(setPage, () => {});
        },
        () => {},
      );
    }, STAMP_EVERY_MS);
    return () => window.clearInterval(timer);
  }, [stamp]);

  // The Activity dock asked for a card: open it once per request.
  const [asked, setAsked] = useState<number>();
  if (openCard && openCard.n !== asked) {
    setAsked(openCard.n);
    setOpen(openCard.id);
  }

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

  async function resume(card: BoardCard) {
    setBusy(true);
    try {
      const started = await workboardResume(card.id, card.chat ?? newConversationId());
      setPage(started.page);
      setNote(null);
      onResume?.({ conversationId: started.conversationId, prompt: started.prompt });
    } catch (err) {
      setNote(String(err));
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
                  {col.cards.map((card, at) => (
                    <BoardCardRow
                      key={card.id}
                      card={card}
                      lane={col.lane}
                      lanes={lanes}
                      at={at}
                      count={col.cards.length}
                      busy={busy}
                      now={now}
                      isOpen={open === card.id}
                      resumable={resumable && Boolean(onResume)}
                      onToggle={() => setOpen(open === card.id ? null : card.id)}
                      onAct={(what, done) => act(card, what, done)}
                      onWork={() => void work(card)}
                      onResume={() => void resume(card)}
                      onDragStart={() => setDragging({ id: card.id, lane: col.lane })}
                      onDragEnd={() => setDragging(null)}
                      onNow={() => setNow(Date.now())}
                      onError={(err) => setNote(String(err))}
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
  lanes: Lanes;
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

/** An opened card: its steps, where it left off, Resume and Hand back for a
 * paused run, Move up and down, and its log, read only when you ask. */
function CardDetail({
  card,
  at,
  count,
  busy,
  now,
  resumable,
  onAct,
  onResume,
  onNow,
  onError,
}: {
  card: BoardCard;
  at: number;
  count: number;
  busy: boolean;
  now: number;
  resumable: boolean;
  onAct: OnAct;
  onResume: () => void;
  onNow: () => void;
  onError: (err: unknown) => void;
}) {
  const [log, setLog] = useState<WorkLogLine[] | null>(null);

  async function readLog() {
    try {
      setLog(await workboardLog(card.id));
      onNow();
    } catch (err) {
      onError(err);
    }
  }

  return (
    <div className="work-open">
      {card.steps.length > 0 && (
        <ol className="work-steps" aria-label="Steps">
          {card.steps.map((step, i) => (
            <li key={i} className={`work-step ${step.status}`}>
              <span className="work-mark" aria-hidden="true">
                {STEP_MARK[step.status]}
              </span>{" "}
              {step.text}
              <span className="sr-only"> ({step.status})</span>
            </li>
          ))}
        </ol>
      )}
      {card.checkpoint && (
        <dl className="work-checkpoint" aria-label="Where it left off">
          {card.checkpoint.goal && (
            <>
              <dt>Goal</dt>
              <dd>{card.checkpoint.goal}</dd>
            </>
          )}
          {card.checkpoint.next && (
            <>
              <dt>Next</dt>
              <dd>{card.checkpoint.next}</dd>
            </>
          )}
          {card.checkpoint.blocker && (
            <>
              <dt>Blocked on</dt>
              <dd>{card.checkpoint.blocker}</dd>
            </>
          )}
          {card.checkpoint.refs.length > 0 && (
            <>
              <dt>Refs</dt>
              <dd>{card.checkpoint.refs.join(" · ")}</dd>
            </>
          )}
        </dl>
      )}
      <div className="card-buttons">
        {card.canResume && (
          <>
            {resumable && (
              <button type="button" disabled={busy} onClick={onResume}>
                Resume
              </button>
            )}
            <button
              type="button"
              disabled={busy}
              title="Make the card yours, back in Todo"
              onClick={() => void onAct({ kind: "handBack" }, "The card is yours now. It is in Todo.")}
            >
              Hand back
            </button>
          </>
        )}
        <button
          type="button"
          className="quiet"
          aria-expanded={log !== null}
          onClick={() => (log === null ? void readLog() : setLog(null))}
        >
          Log
        </button>
        <button
          type="button"
          className="quiet"
          disabled={busy || at <= 0}
          onClick={() => void onAct({ kind: "rank", up: true })}
        >
          Move up
        </button>
        <button
          type="button"
          className="quiet"
          disabled={busy || at >= count - 1}
          onClick={() => void onAct({ kind: "rank", up: false })}
        >
          Move down
        </button>
      </div>
      {log !== null && (
        <div className="work-log" aria-label="Log">
          {log.length === 0 ? (
            <p className="empty">No log yet.</p>
          ) : (
            <ol>
              {log.map((line, i) => (
                <li key={i}>
                  {ago(line.at, now)}
                  {line.kind ? ` · ${line.kind}` : ""} · {line.text}
                </li>
              ))}
            </ol>
          )}
          <button type="button" className="quiet" onClick={() => void readLog()}>
            Refresh
          </button>
        </div>
      )}
    </div>
  );
}

function BoardCardRow({
  card,
  lane,
  lanes,
  at,
  count,
  busy,
  now,
  isOpen,
  resumable,
  onToggle,
  onAct,
  onWork,
  onResume,
  onDragStart,
  onDragEnd,
  onNow,
  onError,
}: {
  card: BoardCard;
  lane: BoardLane;
  lanes: Lanes;
  at: number;
  count: number;
  busy: boolean;
  now: number;
  isOpen: boolean;
  resumable: boolean;
  onToggle: () => void;
  onAct: OnAct;
  onWork: () => void;
  onResume: () => void;
  onDragStart: () => void;
  onDragEnd: () => void;
  onNow: () => void;
  onError: (err: unknown) => void;
}) {
  const [editing, setEditing] = useState<{ title: string; detail: string } | null>(null);
  const [notes, setNotes] = useState<string | null>(null);
  const [done, total] = card.progress;

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

  const meta: string[] = [];
  if (card.leftOff) meta.push(`Left off: ${clip(card.leftOff, LEFT_OFF_MAX)}`);
  if (card.updatedMs > 0) meta.push(`Updated ${ago(card.updatedMs, now)}`);
  if (card.chat) meta.push("Chat");

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
          <button type="button" className="work-head" aria-expanded={isOpen} onClick={onToggle}>
            <span
              className="work-ring"
              role="img"
              aria-label={`${done} of ${total}`}
              style={{ "--fill": total ? done / total : 0 } as CSSProperties}
            />
            <span className="card-title">{card.title}</span>
            {card.fresh && <span className="card-status"> · new</span>}
          </button>
          <div className="work-meta">
            <span className={`work-badge ${card.owner === "Agent" ? "agent" : "you"}`}>{card.owner}</span>
            {card.kind && <span className="work-badge kind">{card.kind}</span>}
            {meta.length > 0 && <span className="card-status">{meta.join(" · ")}</span>}
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
      {isOpen && editing === null && (
        <CardDetail
          card={card}
          at={at}
          count={count}
          busy={busy}
          now={now}
          resumable={resumable}
          onAct={onAct}
          onResume={onResume}
          onNow={onNow}
          onError={onError}
        />
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
