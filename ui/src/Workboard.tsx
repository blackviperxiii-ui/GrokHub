import { useEffect, useState, type CSSProperties } from "react";
import {
  getWorkboard,
  workboardAct,
  workboardLog,
  workboardResume,
  workboardStamp,
  type WorkAct,
  type WorkBoard,
  type WorkCard,
  type WorkColumn,
  type WorkLogLine,
} from "./api";
import { cardsByChat } from "./activity";
import { newConversationId } from "./Chat";
import { ConfirmRow, StatusLine, useNote } from "./ui";

export const WORKBOARD_EMPTY =
  "No work cards yet. Cards from the cabin and from agent runs show up here.";
/** How often the page checks whether the cabin saved the board. */
export const STAMP_EVERY_MS = 3000;
/** Longest "Left off" text on a compact card, like the cabin's meta line. */
const LEFT_OFF_MAX = 60;

const STEP_MARK: Record<WorkCard["steps"][number]["status"], string> = {
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

function allCards(board: WorkBoard): WorkCard[] {
  return [...board.followUp, ...board.columns.flatMap((c) => c.cards), ...board.archived];
}

/** The cabin's Workboard from the same file. Changes go through the shared
 * lock and merge, so neither app overwrites the other; the page reloads when
 * the file changes. In the web app it is read only. Nothing here reaches the
 * model: Resume only drafts a first message for you to send. */
export function Workboard({
  readOnly = false,
  onResume,
  onCards,
  openCard,
}: {
  readOnly?: boolean;
  onResume: (resume: { conversationId: string; prompt: string }) => void;
  /** The chat-to-card map, after every load. */
  onCards?: (byChat: Record<string, string>) => void;
  /** Opens this card, as the Activity dock asks. `n` repeats a request. */
  openCard?: { id: string; n: number };
}) {
  const [board, setBoard] = useState<WorkBoard | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [log, setLog] = useState<{ id: string; lines: WorkLogLine[] } | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const { note, ok, fail } = useNote();

  // The clock for "5m ago": read when the board or a log loads, and each minute.
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = window.setInterval(() => setNow(Date.now()), 60_000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    getWorkboard().then(
      (next) => {
        setBoard(next);
        setNow(Date.now());
      },
      fail,
    );
  }, [fail]);

  useEffect(() => {
    if (board) onCards?.(cardsByChat(board));
  }, [board, onCards]);

  // The cabin may save meanwhile: check the file's mtime, reload on a change.
  const stamp = board?.stamp;
  useEffect(() => {
    if (stamp === undefined) return;
    const timer = window.setInterval(() => {
      workboardStamp().then(
        (current) => {
          if (current !== stamp) getWorkboard().then(setBoard, () => {});
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

  async function act(card: WorkCard, what: WorkAct, done?: string) {
    setBusy(true);
    try {
      setBoard(await workboardAct(card.id, what));
      ok(done ?? null);
    } catch (err) {
      fail(err);
      // The cabin may have changed it; show what the file holds now.
      await getWorkboard().then(setBoard, () => {});
    } finally {
      setBusy(false);
      setConfirm(null);
    }
  }

  async function resume(card: WorkCard) {
    setBusy(true);
    try {
      const started = await workboardResume(card.id, card.chat ?? newConversationId());
      setBoard(started.board);
      onResume({ conversationId: started.conversationId, prompt: started.prompt });
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
    }
  }

  async function readLog(card: WorkCard) {
    try {
      setLog({ id: card.id, lines: await workboardLog(card.id) });
      setNow(Date.now());
    } catch (err) {
      fail(err);
    }
  }

  function toggleLog(card: WorkCard) {
    if (log?.id === card.id) setLog(null);
    else void readLog(card);
  }

  function meta(card: WorkCard) {
    const parts: string[] = [];
    if (card.leftOff) parts.push(`Left off: ${clip(card.leftOff, LEFT_OFF_MAX)}`);
    if (card.updatedMs > 0) parts.push(`Updated ${ago(card.updatedMs, now)}`);
    if (card.chat) parts.push("Chat");
    return parts.join(" · ");
  }

  function deleteRow(card: WorkCard) {
    return confirm === card.id ? (
      <ConfirmRow
        question={`Delete "${card.title}"?`}
        detail="Its log goes too. This can't be undone."
        action="Delete"
        busy={busy}
        onConfirm={() => void act(card, { kind: "delete" }, "Deleted.")}
        onCancel={() => setConfirm(null)}
      />
    ) : null;
  }

  function detail(card: WorkCard, column: WorkColumn | null) {
    const at = column ? column.cards.findIndex((c) => c.id === card.id) : -1;
    return (
      <div className="work-open">
        {card.detail && <p className="work-detail">{card.detail}</p>}
        {card.notes && <p className="work-notes">Notes: {card.notes}</p>}
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
          {!readOnly && card.canResume && (
            <>
              <button type="button" disabled={busy} onClick={() => void resume(card)}>
                Resume
              </button>
              <button
                type="button"
                disabled={busy}
                title="Make the card yours, back in Todo"
                onClick={() => void act(card, { kind: "handBack" }, "The card is yours now. It is in Todo.")}
              >
                Hand back
              </button>
            </>
          )}
          <button type="button" className="quiet" aria-expanded={log?.id === card.id} onClick={() => toggleLog(card)}>
            Log
          </button>
          {!readOnly && column && (
            <>
              <select
                  aria-label="Move to"
                  value={column.id}
                  disabled={busy}
                  onChange={(e) =>
                    void act(card, { kind: "move", column: e.target.value as WorkColumn["id"] })
                  }
                >
                  {board?.columns.map((c) => (
                    <option key={c.id} value={c.id}>
                      {c.label}
                    </option>
                  ))}
              </select>
              <button
                type="button"
                className="quiet"
                disabled={busy || at <= 0}
                onClick={() => void act(card, { kind: "rank", up: true })}
              >
                Move up
              </button>
              <button
                type="button"
                className="quiet"
                disabled={busy || at < 0 || at >= (column?.cards.length ?? 0) - 1}
                onClick={() => void act(card, { kind: "rank", up: false })}
              >
                Move down
              </button>
            </>
          )}
          {!readOnly && (
            <>
              <button
                type="button"
                className="quiet"
                disabled={busy}
                onClick={() => void act(card, { kind: "archive" }, "Archived.")}
              >
                Archive
              </button>
              <button type="button" className="quiet danger" disabled={busy} onClick={() => setConfirm(card.id)}>
                Delete
              </button>
            </>
          )}
        </div>
        {deleteRow(card)}
        {log?.id === card.id && (
          <div className="work-log" aria-label="Log">
            {log.lines.length === 0 ? (
              <p className="empty">No log yet.</p>
            ) : (
              <ol>
                {log.lines.map((line, i) => (
                  <li key={i}>
                    {ago(line.at, now)}
                    {line.kind ? ` · ${line.kind}` : ""} · {line.text}
                  </li>
                ))}
              </ol>
            )}
            <button type="button" className="quiet" onClick={() => void readLog(card)}>
              Refresh
            </button>
          </div>
        )}
      </div>
    );
  }

  function compact(card: WorkCard, column: WorkColumn | null) {
    const [done, total] = card.progress;
    const isOpen = open === card.id;
    return (
      <li key={card.id} className={`card work-card${card.fresh ? " fresh" : ""}`} aria-label={card.title}>
        <button
          type="button"
          className="work-head"
          aria-expanded={isOpen}
          onClick={() => setOpen(isOpen ? null : card.id)}
        >
          <span
            className="work-ring"
            role="img"
            aria-label={`${done} of ${total}`}
            style={{ "--fill": total ? done / total : 0 } as CSSProperties}
          />
          <span className="card-title">{card.title}</span>
        </button>
        <div className="work-meta">
          <span className={`work-badge ${card.owner === "Agent" ? "agent" : "you"}`}>{card.owner}</span>
          {card.kind && <span className="work-badge kind">{card.kind}</span>}
          {meta(card) && <span className="card-status">{meta(card)}</span>}
        </div>
        {isOpen && detail(card, column)}
      </li>
    );
  }

  const cards = board ? allCards(board) : [];
  const onBoard = board ? board.followUp.length + board.columns.reduce((n, c) => n + c.cards.length, 0) : 0;

  return (
    <section className="workboard" aria-label="Workboard">
      <StatusLine note={note} />
      {readOnly && <p className="card-status">The board is read only here. Change it on your computer.</p>}
      {board === null && !note && <p className="empty">Loading the board…</p>}
      {board && cards.length === 0 && <p className="empty">{WORKBOARD_EMPTY}</p>}
      {board && board.followUp.length > 0 && (
        <section className="work-follow" aria-label="Follow up">
          <h3>Follow up ({board.followUp.length})</h3>
          <ul className="rows">{board.followUp.map((c) => compact(c, null))}</ul>
        </section>
      )}
      {board && onBoard > 0 && (
        <div className="work-columns">
          {board.columns.map((column) => (
            <section key={column.id} className="work-column" aria-label={column.label}>
              <h3>
                {column.label} <span className="card-status">{column.cards.length}</span>
              </h3>
              <ul className="rows">{column.cards.map((c) => compact(c, column))}</ul>
            </section>
          ))}
        </div>
      )}
      {board && board.archived.length > 0 && (
        <details className="work-archived">
          <summary>Archived ({board.archived.length})</summary>
          <ul className="rows">
            {board.archived.map((card) => (
              <li key={card.id} className="card work-card done" aria-label={card.title}>
                <span className="card-title">{card.title}</span>
                {!readOnly && (
                  <div className="card-buttons">
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => void act(card, { kind: "restore" }, "Restored to Todo.")}
                    >
                      Restore
                    </button>
                    <button type="button" className="quiet danger" disabled={busy} onClick={() => setConfirm(card.id)}>
                      Delete
                    </button>
                  </div>
                )}
                {deleteRow(card)}
              </li>
            ))}
          </ul>
        </details>
      )}
    </section>
  );
}
