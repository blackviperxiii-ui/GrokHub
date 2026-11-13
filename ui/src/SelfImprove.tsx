// Settings → Behavior → self-improvement, as the cabin has it: Quiet
// self-review with what it logged, the self-improvement history with Revert
// on the newest kept change, runs by area, and saved code ideas with Copy as
// GitHub issue. A code idea is never applied.
import { useEffect, useState } from "react";
import {
  getSelfImprove,
  selfImproveIssue,
  selfImproveRevert,
  setSelfImproveQuiet,
  type SelfImproveView,
} from "./selfImproveApi";
import { Toggle } from "./Toggle";
import { ConfirmRow } from "./ui";

export const QUIET_ROW = "Quiet self-review";
export const QUIET_HINT =
  "The Sunday self-review logs its suggestions here instead of posting Pulse cards. Nothing is applied either way.";
export const HISTORY_EMPTY = "No self-changes yet. The Sunday self-review proposes them as Pulse cards.";

/** Plain rows, no bullets: the cabin lists these as settings lines. */
const ROWS = { listStyle: "none", margin: 0, padding: 0, display: "grid", gap: 8 } as const;

export function SelfImprove() {
  const [view, setView] = useState<SelfImproveView | null>(null);
  const [asking, setAsking] = useState<string | null>(null);
  const [note, setNote] = useState<string | null>(null);

  useEffect(() => {
    getSelfImprove().then(setView, () => setView(null));
  }, []);

  if (!view) return null;

  function quiet(on: boolean) {
    setSelfImproveQuiet(on).then(
      (v) => {
        setView(v);
        setNote("Saved");
      },
      (err) => setNote(String(err)),
    );
  }

  function revert(version: string) {
    setAsking(null);
    selfImproveRevert(version).then(
      (line) => {
        setNote(line);
        getSelfImprove().then(setView, () => {});
      },
      (err) => setNote(String(err)),
    );
  }

  function copyIssue(title: string) {
    selfImproveIssue(title).then(
      async ({ text, copied }) => {
        await navigator.clipboard.writeText(text);
        setNote(copied);
      },
      (err) => setNote(String(err)),
    );
  }

  const most = Math.max(1, ...view.bars.map((b) => b.runs));
  return (
    <section className="self-improve" aria-label="Self-improvement">
      <Toggle checked={view.quiet} onChange={quiet}>
        {QUIET_ROW}
      </Toggle>
      <p className="note">{QUIET_HINT}</p>
      {view.quietNote && <p className="note self-improve-log" style={{ whiteSpace: "pre-line" }}>
          {view.quietNote}
        </p>}
      <h2>Self-improvement history</h2>
      {view.history.length === 0 && <p className="note">{HISTORY_EMPTY}</p>}
      {view.bars.length > 0 && (
        <ul className="self-improve-bars" style={ROWS} aria-label="Runs by area">
          {view.bars.map((b) => (
            <li key={b.domain}>
              <span className="card-status">{b.domain}</span>
              <span
                className="self-improve-bar"
                aria-hidden="true"
                style={{
                  display: "inline-block",
                  height: 8,
                  marginLeft: 8,
                  width: `${Math.round((b.runs / most) * 160)}px`,
                  background: "var(--accent)",
                }}
              />
            </li>
          ))}
        </ul>
      )}
      {view.domains && <p className="note self-improve-log" style={{ whiteSpace: "pre-line" }}>
          {view.domains}
        </p>}
      {view.history.length > 0 && (
        <ul className="self-improve-history" style={ROWS} aria-label="Self-improvement history">
          {view.history.map((r) => (
            <li key={`${r.title}|${r.hint}`}>
              <strong>{r.title}</strong>
              <p className="card-status">{r.hint}</p>
              {r.revert &&
                (asking === r.revert ? (
                  <ConfirmRow
                    question={`Revert ${r.title.replace(/^Kept: /, "")}?`}
                    detail="The version before it comes back."
                    action="Revert"
                    onConfirm={() => revert(r.revert ?? "")}
                    onCancel={() => setAsking(null)}
                  />
                ) : (
                  <button type="button" onClick={() => setAsking(r.revert)}>
                    Revert
                  </button>
                ))}
            </li>
          ))}
        </ul>
      )}
      {view.ideas.length > 0 && (
        <ul className="self-improve-ideas" style={ROWS} aria-label="Saved ideas">
          {view.ideas.map((i) => (
            <li key={i.title}>
              <strong>Idea: {i.title}</strong>
              <p className="card-status">{i.text}</p>
              <button type="button" className="quiet" onClick={() => copyIssue(i.title)}>
                Copy as GitHub issue
              </button>
            </li>
          ))}
        </ul>
      )}
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
    </section>
  );
}
