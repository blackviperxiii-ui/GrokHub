import { useEffect, useState, type FormEvent } from "react";
import {
  getJournal,
  journalAct,
  journalAdd,
  journalToMission,
  type JournalAct,
  type JournalMission,
  type JournalPage,
  type JournalQuestion,
} from "./api";
import { newConversationId } from "./Chat";
import { StatusLine, useNote } from "./ui";

export const JOURNAL_EMPTY =
  "No open questions. Add a decision you're weighing and I'll help you test it.";

/** The cabin's decision journal, from the same file. `onMission` puts the
 * mission's first message in its chat; nothing is sent until you send it. */
export function Journal({ onMission }: { onMission: (mission: JournalMission) => void }) {
  const [page, setPage] = useState<JournalPage | null>(null);
  const [question, setQuestion] = useState("");
  const [options, setOptions] = useState<Record<string, string>>({});
  const { note, ok, fail } = useNote();
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    getJournal().then(setPage, fail);
  }, []);

  async function run(work: () => Promise<JournalPage>, done?: string) {
    setBusy(true);
    try {
      setPage(await work());
      ok(done ?? null);
      return true;
    } catch (err) {
      fail(err);
      // The cabin may have changed it; show what the file holds now.
      await getJournal().then(setPage, () => {});
      return false;
    } finally {
      setBusy(false);
    }
  }

  async function add(event: FormEvent) {
    event.preventDefault();
    if (!question.trim() || busy) return;
    if (await run(() => journalAdd(question), "Added.")) setQuestion("");
  }

  async function act(q: JournalQuestion, what: JournalAct, done?: string) {
    return run(() => journalAct(q.id, what), done);
  }

  async function addOption(event: FormEvent, q: JournalQuestion) {
    event.preventDefault();
    const label = (options[q.id] ?? "").trim();
    if (!label || busy) return;
    if (await act(q, { kind: "addOption", label })) {
      setOptions((now) => ({ ...now, [q.id]: "" }));
    }
  }

  async function mission(q: JournalQuestion) {
    setBusy(true);
    try {
      const started = await journalToMission(q.id, q.mission ?? newConversationId());
      onMission(started);
    } catch (err) {
      fail(err);
    } finally {
      setBusy(false);
      await getJournal().then(setPage, () => {});
    }
  }

  function leaningLabel(q: JournalQuestion) {
    return q.options.find((o) => o.id === q.leaning)?.label ?? null;
  }

  return (
    <section className="journal" aria-label="Journal">
      <form className="journal-add" onSubmit={(e) => void add(e)}>
        <label>
          A decision you're weighing
          <input
            value={question}
            placeholder="Should we ship the shell before the cabin?"
            onChange={(e) => setQuestion(e.target.value)}
          />
        </label>
        <button type="submit" disabled={busy || !question.trim()}>
          Add question
        </button>
      </form>
      <StatusLine note={note} />
      {page === null && !note && <p className="empty">Loading the journal…</p>}
      {page && page.active.length === 0 && <p className="empty">{JOURNAL_EMPTY}</p>}
      {page && (
        <ul className="rows">
          {page.active.map((q) => (
            <li key={q.id} className="card question" aria-label={q.question}>
              <div className="card-status">
                {q.status}
                {leaningLabel(q) && ` · leaning ${leaningLabel(q)}`}
              </div>
              <div className="card-title">{q.question}</div>
              {q.options.length > 0 && (
                <div className="choices" role="group" aria-label="Options">
                  {q.options.map((o) => (
                    <button
                      key={o.id}
                      type="button"
                      aria-pressed={q.leaning === o.id}
                      title="Lean to this option"
                      disabled={busy}
                      onClick={() => void act(q, { kind: "lean", optionId: o.id })}
                    >
                      {o.label}
                    </button>
                  ))}
                </div>
              )}
              {q.options.length < 5 && q.status !== "Deferred" && (
                <form className="journal-option" onSubmit={(e) => void addOption(e, q)}>
                  <input
                    aria-label="New option"
                    placeholder="Another option"
                    value={options[q.id] ?? ""}
                    onChange={(e) => setOptions((now) => ({ ...now, [q.id]: e.target.value }))}
                  />
                  <button type="submit" disabled={busy || !(options[q.id] ?? "").trim()}>
                    Add option
                  </button>
                </form>
              )}
              <div className="card-buttons">
                {q.status === "Deferred" ? (
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void act(q, { kind: "reopen" })}
                  >
                    Reopen
                  </button>
                ) : (
                  <>
                    <button type="button" disabled={busy} onClick={() => void mission(q)}>
                      {q.mission ? "Back to mission" : "To mission"}
                    </button>
                    <button
                      type="button"
                      disabled={busy || q.leaning === null}
                      title={q.leaning === null ? "Lean to an option first" : undefined}
                      onClick={() =>
                        void act(
                          q,
                          { kind: "decide", optionId: q.leaning },
                          `Decided: ${leaningLabel(q)}.`,
                        )
                      }
                    >
                      Decide
                    </button>
                    <button
                      type="button"
                      className="quiet"
                      disabled={busy}
                      onClick={() => void act(q, { kind: "defer" }, "Deferred.")}
                    >
                      Defer
                    </button>
                  </>
                )}
                <button
                  type="button"
                  className="quiet"
                  disabled={busy}
                  onClick={() => void act(q, { kind: "abandon" }, "Abandoned.")}
                >
                  Abandon
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}
      {page && page.history.length > 0 && (
        <details className="journal-history">
          <summary>History ({page.history.length})</summary>
          <ul className="rows">
            {page.history.map((q) => (
              <li key={q.id} className="card question done" aria-label={q.question}>
                <div className="card-status">
                  {q.status}
                  {q.chosen && ` · ${q.chosen}`}
                </div>
                <div className="card-title">{q.question}</div>
              </li>
            ))}
          </ul>
        </details>
      )}
    </section>
  );
}
