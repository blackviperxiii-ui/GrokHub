import { Fragment, useEffect, useState, type MouseEvent } from "react";
import { ConfirmRow } from "./ui";
import {
  allowScope,
  forgetScopeFacts,
  getScopes,
  pickScopeFolder,
  revokeScope,
  type ScopeAnswer,
  type ScopeRow,
  type ScopesView,
} from "./scopesApi";

export const CHOOSE_FOLDER = "Choose folder…";
export const FORGET_THESE = "Forget these";
export const NO_FOLDER_DIALOG = "No folder dialog opened on this computer. Type the folder's path instead.";

/** Every row the same width, its words and fields on the left, its pill on the right. */
const ROW = { alignSelf: "stretch", maxWidth: "none", alignItems: "center" } as const;
const BODY = { flex: 1, minWidth: 0, display: "grid", gap: 6 } as const;
const HINT = { margin: 0 } as const;
/** One learned fact: a single line, cut with an ellipsis like the cabin's. */
const FACT = { margin: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" } as const;

/** A dialog that answers this fast never opened. */
const NO_DIALOG_WITHIN_MS = 400;

/** Allow counts only from the pointer: Enter or Space on a focused Allow does nothing. */
function pointer(e: MouseEvent) {
  return e.detail > 0;
}

/** Settings → Permissions, "What Loadout can read": one row per learning
 * scope, every one off until allowed here. The same ledger as the cabin. */
export function Scopes() {
  const [view, setView] = useState<ScopesView | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [folder, setFolder] = useState("");
  const [browser, setBrowser] = useState("firefox");
  const [busy, setBusy] = useState(false);
  const [picking, setPicking] = useState(false);
  const [forgetting, setForgetting] = useState<string | null>(null);

  useEffect(() => {
    getScopes().then(setView, (err) => setStatus(String(err)));
  }, []);

  async function run(work: () => Promise<ScopeAnswer>, after?: () => void) {
    setBusy(true);
    setForgetting(null);
    try {
      const got = await work();
      setView(got.view);
      setStatus(got.status);
      after?.();
    } catch (err) {
      setStatus(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function choose() {
    setPicking(true);
    const start = Date.now();
    try {
      const dir = await pickScopeFolder();
      if (dir) {
        setFolder(dir);
        setStatus(null);
      } else if (Date.now() - start < NO_DIALOG_WITHIN_MS) {
        setStatus(NO_FOLDER_DIALOG);
      }
    } catch (err) {
      setStatus(String(err));
    } finally {
      setPicking(false);
    }
  }

  function allow(e: MouseEvent, row: ScopeRow) {
    if (!pointer(e) || busy) return;
    const value = row.kind === "files" ? folder : row.kind === "browser_history" ? browser : "";
    void run(
      () => allowScope(row.kind, value),
      () => row.kind === "files" && setFolder(""),
    );
  }

  if (!view) return status ? <p className="note">{status}</p> : null;
  const locked = view.locked !== null;
  const hover = view.lockHover ?? undefined;

  return (
    <section aria-label={view.heading}>
      <h2>{view.heading}</h2>
      <p className="note">{view.note}</p>
      {view.locked && <p className="note">{view.locked}</p>}
      {status && !locked && (
        <p className="note" role="status">
          {status}
        </p>
      )}
      <ul className="rows" aria-label={view.heading}>
        {view.rows.map((row) => (
          <Fragment key={row.kind}>
            {row.granted.map((g) => (
              <li key={g.id} className="card" style={ROW} aria-label={g.title}>
                <div style={BODY}>
                  <div className="card-title">{g.title}</div>
                  <p className="note" style={HINT} title={g.detail ?? undefined}>
                    {g.hint}
                  </p>
                  {g.learned.note && (
                    <p className="note" style={HINT}>
                      {g.learned.note}
                    </p>
                  )}
                  {g.learned.head && (
                    <div aria-label={`Learned from ${g.title}`}>
                      <p style={HINT}>{g.learned.head}</p>
                      {g.learned.lines.map((l) => (
                        <p key={l} className="note" style={FACT}>
                          {l}
                        </p>
                      ))}
                      {g.learned.more && (
                        <p className="note" style={HINT}>
                          {g.learned.more}
                        </p>
                      )}
                    </div>
                  )}
                  {forgetting === g.id && (
                    <ConfirmRow
                      question={`Forget what ${g.title} taught?`}
                      detail="It stays allowed; revoke it to stop reading."
                      action="Forget"
                      busy={busy}
                      onConfirm={() => void run(() => forgetScopeFacts(g.id))}
                      onCancel={() => setForgetting(null)}
                    />
                  )}
                </div>
                <div className="card-buttons">
                  {g.learned.canForget && forgetting !== g.id && (
                    <button
                      type="button"
                      className="quiet"
                      disabled={busy || locked}
                      title={hover}
                      onClick={() => setForgetting(g.id)}
                    >
                      {FORGET_THESE}
                    </button>
                  )}
                  <button
                    type="button"
                    className="quiet"
                    disabled={busy || locked}
                    title={hover}
                    onClick={() => void run(() => revokeScope(g.id))}
                  >
                    Revoke
                  </button>
                </div>
              </li>
            ))}
            {row.add && (
              <li className="card" style={ROW} aria-label={row.add.title}>
                <div style={BODY}>
                  <div className="card-title">{row.add.title}</div>
                  <p className="note" style={HINT}>
                    {row.add.hint}
                  </p>
                  {row.kind === "files" && (
                    <div className="labs-row">
                      <button
                        type="button"
                        className="quiet"
                        disabled={picking || locked}
                        title={hover}
                        onClick={() => void choose()}
                      >
                        {CHOOSE_FOLDER}
                      </button>
                      <input
                        aria-label="Folder"
                        value={folder}
                        placeholder={view.folderHint}
                        disabled={locked}
                        onChange={(e) => setFolder(e.target.value)}
                      />
                    </div>
                  )}
                  {row.kind === "browser_history" && (
                    <div className="labs-row">
                      <select
                        aria-label="Browser"
                        value={browser}
                        disabled={locked}
                        onChange={(e) => setBrowser(e.target.value)}
                      >
                        {view.browsers.map(([id, name]) => (
                          <option key={id} value={id}>
                            {name}
                          </option>
                        ))}
                      </select>
                    </div>
                  )}
                </div>
                <div className="card-buttons">
                  <button type="button" disabled={busy || locked} title={hover} onClick={(e) => allow(e, row)}>
                    Allow
                  </button>
                </div>
              </li>
            )}
          </Fragment>
        ))}
      </ul>
    </section>
  );
}
