import { useEffect, useState } from "react";
import {
  getKnowMe,
  knowMeDelete,
  knowMeScan,
  knowMeWipe,
  setKnowMeOn,
  setKnowMeSignal,
  type KnowMeAnswer,
  type KnowMeView,
} from "./knowMeApi";
import { Toggle } from "./Toggle";
import { ConfirmRow } from "./ui";

export const KNOW_ME_INTRO =
  "Learn your games, hobbies and setup from names and counts on this computer, never file contents, browser data, mail, messages or keys. Only the tags below are kept.";
export const KNOW_ME_MEMORY_NOTE =
  "What chats teach goes to USER.md (you), SOUL.md (how your assistant behaves) and MEMORY.md (what you asked it to remember).";
export const KNOW_ME_SWITCH = "Know me from this computer";
export const KNOWS_HEAD = "What I know about you";
export const NOTHING_YET = "Nothing yet. Scan now fills this in.";

const LIST = { listStyle: "none", margin: 0, padding: 0, display: "grid", gap: 6 } as const;
const SIGNALS = { ...LIST, paddingLeft: 24 } as const;
const TAG = { display: "flex", gap: 8, alignItems: "center", flexWrap: "wrap" } as const;

/** Settings → Pulse, "Know me": tags learned from names and counts on this
 * computer, from the same file as the cabin. Delete and Wipe all ask first. */
export function KnowMe() {
  const [view, setView] = useState<KnowMeView | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [scanning, setScanning] = useState(false);
  const [asking, setAsking] = useState<string | null>(null);

  useEffect(() => {
    getKnowMe().then(setView, (err) => setStatus(String(err)));
  }, []);

  async function run(work: () => Promise<KnowMeAnswer>, scan = false) {
    setBusy(true);
    setScanning(scan);
    setAsking(null);
    try {
      const got = await work();
      setView(got.view);
      setStatus(got.status || null);
    } catch (err) {
      setStatus(String(err));
    } finally {
      setBusy(false);
      setScanning(false);
    }
  }

  if (!view) return status ? <p className="note">{status}</p> : null;

  return (
    <section aria-label="Know me">
      <h2>Know me</h2>
      <p className="note">{KNOW_ME_INTRO}</p>
      <p className="note">{KNOW_ME_MEMORY_NOTE}</p>
      <Toggle checked={view.on} disabled={busy} onChange={(on) => void run(() => setKnowMeOn(on), on)}>
        {KNOW_ME_SWITCH}
      </Toggle>
      {view.on && (
        <>
          <ul style={SIGNALS} aria-label="What it looks at">
            {view.signals.map((s) => (
              <li key={s.id}>
                <Toggle checked={s.on} disabled={busy} onChange={(on) => void run(() => setKnowMeSignal(s.id, on))}>
                  <span title={s.reads}>{s.label}</span>
                </Toggle>
              </li>
            ))}
          </ul>
          <div className="labs-row">
            {scanning ? (
              <p className="note">Scanning…</p>
            ) : (
              <button
                type="button"
                className="quiet"
                disabled={busy}
                onClick={() => void run(knowMeScan, true)}
              >
                Scan now
              </button>
            )}
          </div>
          {view.lastScan && <p className="note">{view.lastScan}</p>}
          <h3>{KNOWS_HEAD}</h3>
          {view.tags.length === 0 && <p className="note">{NOTHING_YET}</p>}
          <ul style={LIST} aria-label={KNOWS_HEAD}>
            {view.tags.map((t) => (
              <li key={t.tag} aria-label={t.tag}>
                <div style={TAG}>
                  <span>{t.tag}</span>
                  <span className="note">{t.from}</span>
                  {asking !== t.tag && (
                    <button
                      type="button"
                      className="quiet"
                      disabled={busy}
                      onClick={() => setAsking(t.tag)}
                    >
                      Delete
                    </button>
                  )}
                </div>
                {asking === t.tag && (
                  <ConfirmRow
                    question={`Delete ${t.tag}?`}
                    detail="A scan won't bring it back. A Pulse topic it added goes too; one you added stays."
                    action="Delete"
                    busy={busy}
                    onConfirm={() => void run(() => knowMeDelete(t.tag))}
                    onCancel={() => setAsking(null)}
                  />
                )}
              </li>
            ))}
          </ul>
          {view.tags.length > 0 &&
            (asking === "*wipe" ? (
              <ConfirmRow
                question="Wipe everything Know me learned?"
                detail="Know me turns off and every tag goes, with the Pulse topics it added."
                action="Wipe all"
                busy={busy}
                onConfirm={() => void run(knowMeWipe)}
                onCancel={() => setAsking(null)}
              />
            ) : (
              <div className="labs-row">
                <button
                  type="button"
                  className="quiet"
                  disabled={busy}
                  onClick={() => setAsking("*wipe")}
                >
                  Wipe all
                </button>
              </div>
            ))}
        </>
      )}
      {status && (
        <p className="note" role="status">
          {status}
        </p>
      )}
    </section>
  );
}
