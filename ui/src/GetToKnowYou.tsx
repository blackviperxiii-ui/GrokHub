// Get to know you: the stage after Auto Setup's first run. A few short,
// skippable screens draft USER.md, SOUL.md and MEMORY.md (and Personal rows
// when AMR is on); nothing is written until Review's Save. It resumes where
// the person left off, and Settings → Personal opens it again.
import { useCallback, useEffect, useRef, useState } from "react";
import { PRODUCT_NAME } from "./brand";
import {
  EMPTY_ANSWERS,
  OPEN_EVENT,
  onboard,
  type Answers,
  type Drafts,
  type OnboardAct,
  type OnboardReply,
  type Step,
} from "./onboardApi";

export const WELCOME_TITLE = `Help ${PRODUCT_NAME} get to know you`;
export const WELCOME_BODY =
  "About two minutes. You see and can edit everything before it's saved, and you can skip any part. It all stays on this computer.";
export const REVIEW_NOTE = "This is exactly what Save writes. Edit or delete anything first.";
export const DONE_NOTE = "Edit any of it later in Settings → Personal.";

const TONES = [
  { value: "friendly", label: "Friendly" },
  { value: "direct", label: "Straight to the point" },
  { value: "playful", label: "Playful" },
] as const;

const GROUP_NAMES: Record<string, string> = {
  about: "About you",
  likes: "What you like",
  routines: "Your routines",
  needs: "What you need",
  thinking: "How you like things done",
};

const ORDER: Step[] = ["welcome", "about", "soul", "life", "review", "done"];

/** The stage. `after` is true once Auto Setup's first-run window is closed:
 * it never shows over that window. */
export function OnboardStage({ after }: { after: boolean }) {
  const [reply, setReply] = useState<OnboardReply | null>(null);
  const [step, setStep] = useState<Step>("welcome");
  const [answers, setAnswers] = useState<Answers>(EMPTY_ANSWERS);
  const [drafts, setDrafts] = useState<Drafts | null>(null);
  const [error, setError] = useState<string | null>(null);
  const box = useRef<HTMLDivElement>(null);
  const shown = after && reply?.pending === true;

  const run = useCallback(async (a: OnboardAct) => {
    try {
      const next = await onboard(a);
      setError(null);
      if (!next) return null;
      setReply(next);
      if (next.drafts) setDrafts(next.drafts);
      return next;
    } catch (err) {
      setError(String(err));
      return null;
    }
  }, []);

  /** Show what the shell said: its step and answers, and Review's drafts. */
  const take = useCallback((next: OnboardReply | null) => {
    if (!next) return;
    setReply(next);
    setStep(next.step === "done" ? "welcome" : next.step);
    setAnswers({ ...EMPTY_ANSWERS, ...next.answers });
    if (next.step === "review") {
      onboard({ act: "draft", answers: next.answers }).then(
        (r) => r?.drafts && setDrafts(r.drafts),
        (err) => setError(String(err)),
      );
    }
  }, []);

  useEffect(() => {
    if (!after) return;
    onboard({ act: "view" }).then(take, (err) => setError(String(err)));
  }, [after, take]);

  useEffect(() => {
    const onOpen = () => {
      onboard({ act: "reopen" }).then(take, (err) => setError(String(err)));
    };
    window.addEventListener(OPEN_EVENT, onOpen);
    return () => window.removeEventListener(OPEN_EVENT, onOpen);
  }, [take]);

  // Each step starts with focus in the window, so Escape and Tab work there.
  useEffect(() => {
    if (shown) box.current?.focus();
  }, [shown, step]);

  if (!shown || !reply) return null;

  const go = (to: Step) => {
    setStep(to);
    if (to === "review") void run({ act: "draft", answers });
    else void run({ act: "keep", step: to, answers });
  };
  const skip = () => {
    setStep("welcome");
    void run({ act: "skip" });
  };
  const set = (key: keyof Answers) => (value: string) => setAnswers((a) => ({ ...a, [key]: value }));
  const at = ORDER.indexOf(step);
  const back = at > 1 && step !== "done" ? ORDER[at - 1] : null;
  const next = ORDER[at + 1];

  let title: string;
  let body;
  switch (step) {
    case "welcome":
      title = WELCOME_TITLE;
      body = <p>{WELCOME_BODY}</p>;
      break;
    case "about":
      title = "About you";
      body = (
        <>
          <Field label="Your name" value={answers.name} onChange={set("name")} />
          <Field label="What should I call you?" value={answers.callMe} onChange={set("callMe")} />
          <Field label="Anything I should know about you?" value={answers.about} onChange={set("about")} long />
        </>
      );
      break;
    case "soul":
      title = "How should I sound?";
      body = (
        <>
          <div className="first-run-choices" role="group" aria-label="Tone">
            {TONES.map((t) => (
              <button
                key={t.value}
                type="button"
                className="chip"
                aria-pressed={answers.tone === t.value}
                onClick={() => set("tone")(answers.tone === t.value ? "" : t.value)}
              >
                {t.label}
              </button>
            ))}
          </div>
          <Field
            label="Anything I should always or never do?"
            value={answers.soulNotes}
            onChange={set("soulNotes")}
            long
          />
        </>
      );
      break;
    case "life":
      title = "Your life and work";
      body = (
        <>
          <Field label="Goals you're working on" value={answers.goals} onChange={set("goals")} />
          <Field label="Interests" value={answers.interests} onChange={set("interests")} />
          <Field label="Routines" value={answers.routines} onChange={set("routines")} />
          <Field label="Apps you use" value={answers.apps} onChange={set("apps")} />
          <Field label="How you like things done" value={answers.prefs} onChange={set("prefs")} />
          <Field label="Anything else to remember" value={answers.remember} onChange={set("remember")} long />
        </>
      );
      break;
    case "review":
      title = "Check what I'll save";
      body = drafts ? (
        <Review
          drafts={drafts}
          amr={reply.amr}
          openclaw={reply.openclaw}
          note={reply.note}
          onChange={setDrafts}
          onImport={() => void run({ act: "openclaw", drafts })}
        />
      ) : (
        <p className="note">Drafting…</p>
      );
      break;
    case "done": {
      title = "Saved";
      const s = reply.saved;
      const rows = s && s.rows > 0 ? ` and ${s.rows} Personal ${s.rows === 1 ? "row" : "rows"}` : "";
      body = (
        <p>
          {s && s.files.length > 0 ? `Saved ${s.files.join(", ")}${rows}.` : "Nothing changed."} {DONE_NOTE}
        </p>
      );
      break;
    }
  }

  return (
    <div className="first-run-backdrop">
      <div
        ref={box}
        tabIndex={-1}
        className="card first-run onboard"
        role="dialog"
        aria-modal="true"
        aria-labelledby="onboard-title"
        onKeyDown={(e) => {
          if (e.key === "Escape" && step !== "done") skip();
        }}
      >
        <h2 id="onboard-title">{title}</h2>
        {body}
        {error && <p className="note warn">{error}</p>}
        <div className="first-run-foot">
          {back && (
            <button type="button" className="chip" onClick={() => go(back)}>
              Back
            </button>
          )}
          {step !== "done" && (
            <button type="button" className="chip" onClick={skip}>
              Skip for now
            </button>
          )}
          {step === "welcome" && (
            <button type="button" onClick={() => go("about")}>
              Start
            </button>
          )}
          {(step === "about" || step === "soul" || step === "life") && (
            <button type="button" onClick={() => go(next)}>
              {step === "life" ? "Review" : "Next"}
            </button>
          )}
          {step === "review" && drafts && (
            <button
              type="button"
              onClick={async () => {
                if (await run({ act: "save", drafts })) setStep("done");
              }}
            >
              Save
            </button>
          )}
          {step === "done" && (
            <button type="button" onClick={() => setReply(null)}>
              Done
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

function Field({
  label,
  value,
  onChange,
  long,
}: {
  label: string;
  value: string;
  onChange: (v: string) => void;
  long?: boolean;
}) {
  return (
    <label className="onboard-field">
      <span>{label}</span>
      {long ? (
        <textarea rows={2} value={value} onChange={(e) => onChange(e.target.value)} />
      ) : (
        <input type="text" value={value} onChange={(e) => onChange(e.target.value)} />
      )}
    </label>
  );
}

function Review({
  drafts,
  amr,
  openclaw,
  note,
  onChange,
  onImport,
}: {
  drafts: Drafts;
  amr: boolean;
  openclaw: boolean;
  note: string | null;
  onChange: (d: Drafts) => void;
  onImport: () => void;
}) {
  const file = (key: "user" | "soul" | "memory", label: string) => (
    <label className="onboard-field">
      <span>{label}</span>
      <textarea
        className="code"
        rows={5}
        value={drafts[key]}
        onChange={(e) => onChange({ ...drafts, [key]: e.target.value })}
      />
    </label>
  );
  return (
    <>
      <p className="note">{REVIEW_NOTE}</p>
      {file("user", "About you (USER.md)")}
      {file("soul", "How I sound (SOUL.md)")}
      {file("memory", "Notes to keep (MEMORY.md)")}
      {amr && drafts.rows.length > 0 && (
        <>
          <p className="note">These also go to Settings → Personal:</p>
          <ul className="first-run-list" aria-label="Personal rows">
            {drafts.rows.map((r, i) => (
              <li key={`${r.group}-${i}`}>
                <span>
                  <strong>{r.text}</strong>
                  <span className="note">{GROUP_NAMES[r.group] ?? r.group}</span>
                </span>
                <button
                  type="button"
                  className="chip"
                  aria-label={`Remove: ${r.text}`}
                  onClick={() =>
                    onChange({
                      ...drafts,
                      rows: drafts.rows.filter((_, j) => j !== i),
                    })
                  }
                >
                  Remove
                </button>
              </li>
            ))}
          </ul>
        </>
      )}
      {openclaw && (
        <button type="button" className="chip onboard-import" onClick={onImport}>
          Add from OpenClaw
        </button>
      )}
      {note && <p className="note">{note}</p>}
    </>
  );
}
