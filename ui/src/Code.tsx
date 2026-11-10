import { useCallback, useEffect, useState } from "react";
import {
  codeBuiltins,
  codeDiff,
  codeRepo,
  type CodeBuiltin,
  type CodeDiff,
  type CodeOverriddenBy,
  type CodeRepo,
} from "./api";
import { newConversationId } from "./Chat";

export const CODE_NOT_REPO = "This workspace isn't a git repo yet. Pick a project folder in Settings, or ask the agent to start one.";
export const CODE_CLEAN = "No changes since the last commit.";
export const CODE_NEXT = "Coming next on this tab: editor, terminal, tests, and an agent that drives the work while you watch or step in.";
export const CODE_REVIEW_PROMPT = "Review the uncommitted changes in this workspace: what changed, anything risky, and what to test before committing.";
export const CODE_BUILTINS = "Built-in skills";

/** The label on a built-in row that a same-name skill currently shadows. */
export function overrideLabel(over: CodeOverriddenBy): string {
  return `overridden by ${over.scope} skill ${over.name}`;
}

export interface CodeTask {
  prompt: string;
  conversationId: string;
}

/** The Code tab: the workspace's repo, its changed files and one file's diff, plus built-in skills. Read only. */
export function Code({ onTask }: { onTask: (task: CodeTask) => void }) {
  const [repo, setRepo] = useState<CodeRepo | null>(null);
  const [diff, setDiff] = useState<CodeDiff | null>(null);
  const [builtins, setBuiltins] = useState<CodeBuiltin[] | null>(null);
  const [openSkill, setOpenSkill] = useState<CodeBuiltin | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    setBusy(true);
    setNote(null);
    try {
      const [next, skills] = await Promise.all([codeRepo(), codeBuiltins()]);
      setRepo(next);
      setBuiltins(skills);
      setDiff((open) => (open && next.files.some((f) => f.path === open.path) ? open : null));
      setOpenSkill((open) => (open && skills.some((s) => s.name === open.name) ? skills.find((s) => s.name === open.name) ?? null : null));
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }, []);

  useEffect(() => {
    codeRepo().then(setRepo, (err) => setNote(String(err)));
    codeBuiltins().then(setBuiltins, (err) => setNote(String(err)));
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
      {repo?.isRepo && repo.files.length === 0 && (
        <p className={builtins && builtins.length > 0 ? "note" : "empty"}>{CODE_CLEAN}</p>
      )}
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
      {builtins && builtins.length > 0 && (
        <div className="code-skills">
          <h3>{CODE_BUILTINS}</h3>
          <div className={openSkill ? "code-split" : undefined}>
            <ul className="rows" aria-label={CODE_BUILTINS}>
              {builtins.map((skill) => (
                <li key={skill.name}>
                  <button
                    type="button"
                    className="code-skill quiet"
                    aria-pressed={openSkill?.name === skill.name}
                    onClick={() => setOpenSkill(skill)}
                  >
                    <span className="card-title">{skill.name}</span>
                    {skill.description && <span className="note">{skill.description}</span>}
                    {skill.overriddenBy && (
                      <span className="note">{overrideLabel(skill.overriddenBy)}</span>
                    )}
                  </button>
                </li>
              ))}
            </ul>
            {openSkill && (
              <pre className="code-diff" aria-label={`Built-in skill ${openSkill.name}`}>
                {openSkill.body}
              </pre>
            )}
          </div>
        </div>
      )}
      <p className="note">{CODE_NEXT}</p>
    </section>
  );
}
