import { useCallback, useEffect, useState } from "react";
import { codeDiff, codeRepo, type CodeDiff, type CodeRepo } from "./api";
import { newConversationId } from "./Chat";

export const CODE_NOT_REPO = "This workspace isn't a git repo yet. Pick a project folder in Settings, or ask the agent to start one.";
export const CODE_CLEAN = "No changes since the last commit.";
export const CODE_NEXT = "Coming next on this tab: editor, terminal, tests, and an agent that drives the work while you watch or step in.";
export const CODE_REVIEW_PROMPT = "Review the uncommitted changes in this workspace: what changed, anything risky, and what to test before committing.";

export interface CodeTask {
  prompt: string;
  conversationId: string;
}

/** The Code tab, slice 1: the workspace's repo, its changed files and one file's diff. Read only. */
export function Code({ onTask }: { onTask: (task: CodeTask) => void }) {
  const [repo, setRepo] = useState<CodeRepo | null>(null);
  const [diff, setDiff] = useState<CodeDiff | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    setBusy(true);
    setNote(null);
    try {
      const next = await codeRepo();
      setRepo(next);
      setDiff((open) => (open && next.files.some((f) => f.path === open.path) ? open : null));
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    codeRepo().then(setRepo, (err) => setNote(String(err)));
  }, []);

  async function open(path: string) {
    setNote(null);
    try {
      setDiff(await codeDiff(path));
    } catch (err) {
      setNote(String(err));
    }
  }

  const sync = repo && (repo.ahead || repo.behind) ? ` · ${repo.ahead} ahead, ${repo.behind} behind` : "";
  return (
    <section className="code" aria-label="Code">
      <header className="code-head">
        <div>
          <div className="card-title">{repo?.workspace ?? "Workspace"}</div>
          {repo?.isRepo && (
            <div className="note">
              {repo.branch ? `On ${repo.branch}` : "Detached HEAD"}
              {repo.upstream ? ` → ${repo.upstream}` : ""}
              {sync}
            </div>
          )}
        </div>
        <div className="card-buttons">
          <button type="button" onClick={() => void load()} disabled={busy}>
            {busy ? "Refreshing…" : "Refresh"}
          </button>
          {repo?.isRepo && repo.files.length > 0 && (
            <button
              type="button"
              onClick={() => onTask({ prompt: CODE_REVIEW_PROMPT, conversationId: newConversationId() })}
            >
              Ask the agent to review
            </button>
          )}
        </div>
      </header>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      {repo === null && !note && <p className="empty">Loading the workspace…</p>}
      {repo && !repo.isRepo && <p className="empty">{CODE_NOT_REPO}</p>}
      {repo?.isRepo && repo.files.length === 0 && <p className="empty">{CODE_CLEAN}</p>}
      {repo?.isRepo && repo.files.length > 0 && (
        <div className="code-split">
          <ul className="rows" aria-label="Changed files">
            {repo.files.map((f) => (
              <li key={f.path}>
                <button
                  type="button"
                  className={`code-file ${f.status}`}
                  aria-pressed={diff?.path === f.path}
                  onClick={() => void open(f.path)}
                >
                  <span className="code-status">{f.status}</span> {f.path}
                  {f.staged && <span className="note"> (staged)</span>}
                </button>
              </li>
            ))}
            {repo.more && <li className="note">More files changed than this list shows.</li>}
          </ul>
          {diff && (
            <pre className="code-diff" aria-label={`Diff of ${diff.path}`}>
              {diff.text || "No text changes (binary or mode change)."}
              {diff.cut && "\n… cut here; the file's diff is too long to show in full."}
            </pre>
          )}
        </div>
      )}
      <p className="note">{CODE_NEXT}</p>
    </section>
  );
}
