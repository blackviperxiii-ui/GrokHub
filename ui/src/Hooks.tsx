import { useEffect, useState } from "react";
import { getHooks, setHooksTrust, type HookOrigin, type HookRow, type HooksPage } from "./api";
import { ConfirmRow, StatusLine, useNote } from "./ui";

export const HOOKS_NOTE =
  "Hooks run a command when the chat uses a tool. Yours and your plugins' always run. This project's run only after you trust its folder.";

export const HOOKS_EMPTY =
  "No hooks yet. Add them as JSON files in .grok/hooks, in your home folder or in this project.";

export const TRUSTED_NOTE =
  "You trust this folder. Its hooks run, and the allow rules in its .claude/settings.json apply.";

export const UNTRUSTED_NOTE =
  "You haven't trusted this folder. Its hooks don't run, and the allow rules in its .claude/settings.json don't apply.";

export const TRUST_DETAIL =
  "Its hooks can run any command on this computer while the chat works. Trust only a project you know.";

const ORIGIN: Record<HookOrigin, string> = { user: "Yours", project: "This project", plugin: "Plugin" };

export function hookTitle(row: HookRow): string {
  return `${row.event} · ${row.matcher || "any tool"}`;
}

/** Whose hook it is, then why it doesn't run (from the shell). */
export function hookStatus(row: HookRow): string {
  return row.note ? `${ORIGIN[row.origin]} · ${row.note}` : ORIGIN[row.origin];
}

/** Connectors → Hooks: every hook the chat can run here, and this folder's
 * trust. A project's hooks come from its repository, so they wait for it. */
export function Hooks() {
  const [page, setPage] = useState<HooksPage | null>(null);
  const { note, ok, fail } = useNote();
  const [busy, setBusy] = useState(false);
  const [asking, setAsking] = useState(false);

  useEffect(() => {
    getHooks().then(setPage, fail);
  }, [fail]);

  async function trust(trusted: boolean) {
    setBusy(true);
    setAsking(false);
    try {
      const next = await setHooksTrust(trusted);
      setPage(next);
      ok(next.status);
    } catch (err) {
      fail(err);
      // Show what the trust file holds now.
      await getHooks().then(setPage, () => {});
    } finally {
      setBusy(false);
    }
  }

  return (
    <section className="hooks" aria-label="Hooks">
      <h2>Hooks</h2>
      <p className="note">{HOOKS_NOTE}</p>
      {page && (
        <p className="note">
          This folder: <code>{page.folder}</code>. {page.trusted ? TRUSTED_NOTE : UNTRUSTED_NOTE}
        </p>
      )}
      {page &&
        (asking ? (
          <ConfirmRow
            question="Trust this folder's hooks?"
            detail={TRUST_DETAIL}
            action="Trust"
            busy={busy}
            onConfirm={() => void trust(true)}
            onCancel={() => setAsking(false)}
          />
        ) : (
          <div className="card-buttons">
            {page.trusted ? (
              <button type="button" className="quiet" disabled={busy} onClick={() => void trust(false)}>
                Stop trusting
              </button>
            ) : (
              <button type="button" disabled={busy} onClick={() => setAsking(true)}>
                Trust this folder's hooks…
              </button>
            )}
          </div>
        ))}
      <StatusLine note={note} />
      {page === null && !note && <p className="empty">Loading hooks…</p>}
      {page?.rows.length === 0 && <p className="empty">{HOOKS_EMPTY}</p>}
      {page && page.rows.length > 0 && (
        <ul className="rows" aria-label="Hooks list">
          {page.rows.map((row, i) => (
            <li key={`${i}-${row.path}`} className="card hook" aria-label={`${hookTitle(row)} ${row.command}`}>
              <div className="card-title">{hookTitle(row)}</div>
              <div className="card-detail-line">
                <code>{row.command}</code>
              </div>
              <div className="card-status">{hookStatus(row)}</div>
              <div className="card-detail-line">{row.path}</div>
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
