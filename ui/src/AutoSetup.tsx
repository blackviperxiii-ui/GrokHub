// Auto setup on the page: the first-run window, the note in the corner,
// Settings → Auto setup, and Settings → Appearance's text size and motion.
import { useCallback, useEffect, useRef, useState } from "react";
import { autoSetup, TEXT_STEPS, type Act, type AutoReply, type Line, type Note } from "./autoSetupApi";
import { PRODUCT_NAME } from "./brand";
import { Toggle } from "./Toggle";
import { useModalFocus } from "./ui";

/** Away this long, then back: a good moment for a visible change. */
export const AWAY_MS = 30 * 60 * 1000;
/** A look at the PC this often, for power and screen changes. */
export const LOOK_MS = 10 * 60 * 1000;
/** A "changed" note goes away on its own after this. */
export const NOTE_MS = 20 * 1000;

export const WELCOME = `${PRODUCT_NAME} can set itself up for this PC and for you. It changes nothing you can't undo.`;
export const CHECKING = "Checking this PC…";
export const SUMMARY = "Here's what I set up. Undo puts any of it back.";
export const PAGE_NOTE = `${PRODUCT_NAME} sets these for this PC and you, and keeps tuning them. Undo puts one back and leaves it to you.`;

/** Auto setup for the window: a look at launch, every few minutes and on the
 * way back from a break. The note shows once per reply that has one. */
export function useAutoSetup() {
  const [reply, setReply] = useState<AutoReply | null>(null);
  const [note, setNote] = useState<Note | null>(null);
  const hiddenAt = useRef<number | null>(null);

  const take = useCallback((next: AutoReply | null) => {
    if (!next) return;
    setReply(next);
    if (next.note) setNote(next.note);
  }, []);
  const act = useCallback(
    (a: Act) => {
      autoSetup(a).then(take, () => {});
    },
    [take],
  );

  useEffect(() => {
    act({ act: "pass", back: false });
    const timer = window.setInterval(() => act({ act: "pass", back: false }), LOOK_MS);
    const onVisible = () => {
      if (document.hidden) {
        hiddenAt.current = Date.now();
        return;
      }
      const away = hiddenAt.current !== null && Date.now() - hiddenAt.current >= AWAY_MS;
      hiddenAt.current = null;
      act({ act: "pass", back: away });
    };
    document.addEventListener("visibilitychange", onVisible);
    return () => {
      window.clearInterval(timer);
      document.removeEventListener("visibilitychange", onVisible);
    };
  }, [act]);

  // A change note leaves on its own; an offer waits for Yes or Not now.
  useEffect(() => {
    if (!note || note.offer) return;
    const timer = window.setTimeout(() => setNote(null), NOTE_MS);
    return () => window.clearTimeout(timer);
  }, [note]);

  return { reply, note, act, closeNote: () => setNote(null) };
}

/** The first-run window: welcome, a few questions, then what it set. */
export function FirstRun({ reply, act }: { reply: AutoReply; act: (a: Act) => void }) {
  const [started, setStarted] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  useModalFocus(box);
  let body;
  if (reply.summary) {
    body = <Summary reply={reply} act={act} />;
  } else if (reply.question) {
    const q = reply.question;
    body = (
      <>
        <h2 id="first-run-title">{q.prompt}</h2>
        <div className="first-run-choices" role="group" aria-label={q.prompt}>
          {q.choices.map((c) => (
            <button key={c.value} type="button" onClick={() => act({ act: "answer", id: q.id, value: c.value })}>
              {c.label}
            </button>
          ))}
        </div>
        <div className="first-run-foot">
          <button type="button" className="chip" onClick={() => act({ act: "answer", id: q.id })}>
            Skip this one
          </button>
        </div>
      </>
    );
  } else if (started) {
    body = <h2 id="first-run-title">{CHECKING}</h2>;
  } else {
    body = (
      <>
        <h2 id="first-run-title">Welcome to {PRODUCT_NAME}</h2>
        <p>{WELCOME}</p>
        <div className="first-run-bigger">
          <button type="button" className="first-run-big" onClick={() => act({ act: "bigger" })}>
            Make things bigger
          </button>
          <span className="note">Text size: {reply.textSize}%</span>
        </div>
        <div className="first-run-foot">
          <button type="button" className="chip" onClick={() => act({ act: "finish" })}>
            Skip setup
          </button>
          <button
            type="button"
            className="chip"
            onClick={() => {
              setStarted(true);
              act({ act: "start", pace: "detailed" });
            }}
          >
            Tailor it (about 30 s)
          </button>
          <button
            type="button"
            onClick={() => {
              setStarted(true);
              act({ act: "start", pace: "quick" });
            }}
          >
            Just set it up
          </button>
        </div>
      </>
    );
  }
  return (
    <div className="first-run-backdrop">
      <div
        ref={box}
        className="card first-run"
        role="dialog"
        aria-modal="true"
        aria-labelledby="first-run-title"
        onKeyDown={(e) => {
          if (e.key === "Escape") act({ act: "finish" });
        }}
      >
        {body}
      </div>
    </div>
  );
}

function Summary({ reply, act }: { reply: AutoReply; act: (a: Act) => void }) {
  const nothing = reply.made.length === 0 && reply.offered.length === 0;
  return (
    <>
      <h2 id="first-run-title">You're set up</h2>
      <p>{nothing ? "Everything already suits this PC." : SUMMARY}</p>
      <ul className="first-run-list">
        {reply.made.map((l) => (
          <LineRow key={l.key} line={l} button="Undo" onPress={() => act({ act: "undo", key: l.key })} />
        ))}
        {reply.offered.map((l) => (
          <LineRow key={l.key} line={l} button="Yes" onPress={() => act({ act: "accept", key: l.key })} />
        ))}
      </ul>
      <div className="first-run-foot">
        <button type="button" onClick={() => act({ act: "finish" })}>
          Done
        </button>
      </div>
    </>
  );
}

function LineRow({ line, button, onPress }: { line: Line; button: string; onPress: () => void }) {
  return (
    <li>
      <span>
        <strong>{line.text}</strong>
        <span className="note">{line.reason}</span>
      </span>
      <button type="button" className="chip" aria-label={`${button}: ${line.text}`} onClick={onPress}>
        {button}
      </button>
    </li>
  );
}

/** The one note in the corner: a change with Undo, or an offer. */
export function AutoNote({ note, act, onClose }: { note: Note; act: (a: Act) => void; onClose: () => void }) {
  const key = note.line.key;
  const press = (a: Act) => {
    onClose();
    act(a);
  };
  return (
    <div className="card auto-note" role="status">
      <strong>{note.line.text}</strong>
      <span className="note">{note.line.reason}</span>
      <div className="auto-note-buttons">
        {note.offer ? (
          <>
            <button type="button" onClick={() => press({ act: "accept", key })}>
              Yes
            </button>
            <button type="button" className="chip" onClick={() => press({ act: "dismiss", key })}>
              Not now
            </button>
          </>
        ) : (
          <>
            <button type="button" onClick={() => press({ act: "undo", key })}>
              Undo
            </button>
            <button type="button" className="chip" onClick={onClose}>
              OK
            </button>
          </>
        )}
      </div>
    </div>
  );
}

/** Settings → Auto setup: what it set, with Undo, and the ways to start over. */
export function AutoSetupPage() {
  const [reply, setReply] = useState<AutoReply | null>(null);
  const [note, setNote] = useState<string | null>(null);

  function run(a: Act, done?: string) {
    autoSetup(a).then(
      (next) => {
        if (next) setReply(next);
        if (done) setNote(done);
      },
      (err) => setNote(String(err)),
    );
  }

  useEffect(() => run({ act: "view" }), []);

  if (!reply) return <p className="note">{note ?? "Loading…"}</p>;
  return (
    <div className="auto-setup" role="group" aria-label="Auto setup">
      <p className="note">{PAGE_NOTE}</p>
      {reply.rows.length === 0 && <p className="note">Nothing set yet.</p>}
      <ul className="auto-setup-rows">
        {reply.rows.map((r) => (
          <li key={r.key}>
            <span>
              <strong>{r.label}</strong>
              <span className="note">{r.text}</span>
            </span>
            {r.undo ? (
              <button type="button" className="chip" aria-label={`Undo ${r.label}`} onClick={() => run({ act: "undo", key: r.key })}>
                Undo
              </button>
            ) : (
              <button
                type="button"
                className="chip"
                aria-label={`Hand back ${r.label}`}
                onClick={() => run({ act: "handBack", key: r.key }, `${r.label} is ${PRODUCT_NAME}'s to tune again.`)}
              >
                Hand back
              </button>
            )}
          </li>
        ))}
      </ul>
      <h2>Start over</h2>
      <p className="note">Let {PRODUCT_NAME} tune every setting you changed yourself again.</p>
      <button type="button" onClick={() => run({ act: "handBackAll" }, `${PRODUCT_NAME} tunes every setting again.`)}>
        Hand everything back
      </button>
      <p className="note">Clears the answers, the change history and what you said no to. Settings stay as they are.</p>
      <button type="button" onClick={() => run({ act: "forget" }, "Forgot what auto setup learned.")}>
        Forget what you learned
      </button>
      {note && <p className="note">{note}</p>}
    </div>
  );
}

/** Settings → Appearance: text size and reduce motion. A pick here is the
 * person's, so auto setup leaves it alone from then on. */
export function LookRows() {
  const [reply, setReply] = useState<AutoReply | null>(null);
  const [note, setNote] = useState<string | null>(null);

  function run(a: Act) {
    autoSetup(a).then(
      (next) => {
        if (next) setReply(next);
        setNote(null);
      },
      (err) => setNote(String(err)),
    );
  }

  useEffect(() => run({ act: "view" }), []);

  if (!reply) return null;
  return (
    <>
      <h2>Text size</h2>
      <p className="note">Makes text and buttons bigger everywhere.</p>
      <div className="chip-group" role="group" aria-label="Text size">
        {TEXT_STEPS.map((pct) => (
          <button
            key={pct}
            type="button"
            className="chip"
            aria-pressed={reply.textSize === pct}
            onClick={() => run({ act: "set", key: "textSize", value: { t: "pct", v: pct } })}
          >
            {pct}%
          </button>
        ))}
      </div>
      <Toggle
        checked={reply.reduceMotion}
        onChange={(on) => run({ act: "set", key: "reduceMotion", value: { t: "bool", v: on } })}
      >
        Reduce motion
      </Toggle>
      {note && <p className="note warn">{note}</p>}
    </>
  );
}
