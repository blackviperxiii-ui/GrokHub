import { lazy, Suspense, useCallback, useEffect, useEffectEvent, useRef, useState } from "react";
import {
  codeBuiltins,
  codeDiff,
  codeRepo,
  codeTestPlan,
  codeTestRun,
  onCodeRun,
  type CodeBuiltin,
  type CodeDiff,
  type CodeOverriddenBy,
  type CodeRepo,
} from "./api";
import { newConversationId } from "./Chat";
import { CodeDeploy } from "./CodeDeploy";
import { CodeShip } from "./CodeShip";
import { inShell } from "./transport";

// CodeMirror and xterm load only when the tab first shows them.
const CodeEditor = lazy(() => import("./CodeEditor"));
const CodeTerminal = lazy(() => import("./CodeTerminal"));

export const CODE_NOT_REPO = "This workspace isn't a git repo yet. Pick a project folder in Settings, or ask the agent to start one.";
export const CODE_CLEAN = "No changes since the last commit.";
export const CODE_NEXT = "Coming next on this tab: an agent that drives the work while you watch or step in.";
export const CODE_DESKTOP_ONLY = "The editor and terminal are only in the desktop app.";
export const CODE_REVIEW_PROMPT = "Review the uncommitted changes in this workspace: what changed, anything risky, and what to test before committing.";
export const CODE_BUILTINS = "Built-in skills";
export const CODE_TESTS = "Tests";
export const CODE_CONFIRM = "Run this test command?";
export const CODE_NO_COMMAND = "Type a test command to run.";

/** The label on a built-in row that a same-name skill currently shadows. */
export function overrideLabel(over: CodeOverriddenBy): string {
  return `overridden by ${over.scope} skill ${over.name}`;
}

export interface CodeTask {
  prompt: string;
  conversationId: string;
}

/** The Code tab: the workspace's repo, its changed files and one file's diff, an editor and a terminal, plus built-in skills and a test runner. */
export function Code({ onTask }: { onTask: (task: CodeTask) => void }) {
  const [repo, setRepo] = useState<CodeRepo | null>(null);
  const [diff, setDiff] = useState<CodeDiff | null>(null);
  const [builtins, setBuiltins] = useState<CodeBuiltin[] | null>(null);
  const [openSkill, setOpenSkill] = useState<CodeBuiltin | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [command, setCommand] = useState("");
  const [needsConfirm, setNeedsConfirm] = useState(true);
  const [confirming, setConfirming] = useState(false);
  const [running, setRunning] = useState(false);
  const [log, setLog] = useState("");
  const [runNote, setRunNote] = useState<string | null>(null);
  const [activityKey, setActivityKey] = useState(0);

  // The file whose diff is on screen, the newest change to that pane, and the
  // newest file picked: an answer to an older one is dropped.
  const openPath = useRef<string | null>(null);
  const diffAsk = useRef(0);
  const picked = useRef(0);

  /** Read the open diff again, or close it when its file is no longer changed. */
  const rereadDiff = useCallback(async (files: CodeRepo["files"]) => {
    const path = openPath.current;
    if (path === null) return;
    const ask = ++diffAsk.current;
    const next = files.some((f) => f.path === path) ? await codeDiff(path).catch(() => null) : null;
    if (ask !== diffAsk.current) return;
    if (!next) openPath.current = null;
    setDiff(next);
  }, []);

  const load = useCallback(async () => {
    setBusy(true);
    setNote(null);
    try {
      const [next, skills] = await Promise.all([codeRepo(), codeBuiltins()]);
      setRepo(next);
      setBuiltins(skills);
      setOpenSkill((open) => (open && skills.some((s) => s.name === open.name) ? skills.find((s) => s.name === open.name) ?? null : null));
      await rereadDiff(next.files);
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }, [rereadDiff]);

  // Runs each time the tab shows. A command typed over the planned one stays;
  // untouched, the box takes the fresh plan.
  const planned = useRef("");
  const showPlan = useEffectEvent((plan: { command: string; needsConfirm: boolean }) => {
    if (command === planned.current) {
      setCommand(plan.command);
      setNeedsConfirm(plan.needsConfirm);
    }
    planned.current = plan.command;
  });

  useEffect(() => {
    codeRepo().then(
      (next) => {
        setRepo(next);
        void rereadDiff(next.files);
      },
      (err) => setNote(String(err)),
    );
    codeBuiltins().then(setBuiltins, (err) => setNote(String(err)));
    codeTestPlan().then(
      (plan) => showPlan(plan),
      (err) => setNote(String(err)),
    );
  }, [rereadDiff]);

  useEffect(() => {
    let gone = false;
    let unlisten = () => {};
    void onCodeRun((chunk) => {
      if (gone || !chunk.text) return;
      setLog((log) => log + chunk.text);
    }).then(
      (off) => {
        if (gone) off();
        else unlisten = off;
      },
      () => {},
    );
    return () => {
      gone = true;
      unlisten();
    };
  }, []);

  async function open(path: string) {
    setNote(null);
    const pick = ++picked.current;
    try {
      const next = await codeDiff(path);
      if (pick !== picked.current) return;
      // Only a file that opened takes the pane: a failed pick leaves the diff
      // on screen, and a re-read of it still out, as they were.
      openPath.current = path;
      diffAsk.current += 1;
      setDiff(next);
    } catch (err) {
      if (pick === picked.current) setNote(String(err));
    }
  }

  async function refreshPlan(nextCommand: string) {
    const plan = await codeTestPlan(nextCommand);
    setNeedsConfirm(plan.needsConfirm);
    return plan;
  }

  async function requestRun() {
    const trimmed = command.trim();
    if (!trimmed || running) return;
    setNote(null);
    setRunNote(null);
    try {
      const plan = await refreshPlan(trimmed);
      setCommand(plan.command);
      if (plan.needsConfirm) {
        setConfirming(true);
        return;
      }
      await doRun(plan.command, false);
    } catch (err) {
      setRunNote(String(err));
    }
  }

  async function confirmRun() {
    const trimmed = command.trim();
    if (!trimmed || running) return;
    setConfirming(false);
    await doRun(trimmed, true);
  }

  async function doRun(cmd: string, confirmed: boolean) {
    setRunning(true);
    setLog("");
    setRunNote(null);
    try {
      const result = await codeTestRun(cmd, confirmed);
      setLog(
        result.output + (result.cut ? "\n… cut here; the output is too long to show in full." : ""),
      );
      setRunNote(result.exit === 0 ? "Tests passed." : `Tests exited with ${result.exit}.`);
      const plan = await codeTestPlan(cmd);
      setNeedsConfirm(plan.needsConfirm);
    } catch (err) {
      setRunNote(String(err));
    } finally {
      setRunning(false);
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
      {repo !== null &&
        (inShell() ? (
          <Suspense fallback={<p className="note">Loading the editor and terminal…</p>}>
            <CodeEditor />
            <CodeTerminal />
          </Suspense>
        ) : (
          <p className="note">{CODE_DESKTOP_ONLY}</p>
        ))}
      <div className="code-tests">
        <h3>{CODE_TESTS}</h3>
        <div className="code-test-row">
          <label>
            Test command
            <input
              value={command}
              spellCheck={false}
              autoComplete="off"
              placeholder="cargo test"
              disabled={running}
              onChange={(e) => {
                setCommand(e.target.value);
                setConfirming(false);
                setNeedsConfirm(true);
              }}
            />
          </label>
          <button type="button" onClick={() => void requestRun()} disabled={running || !command.trim()}>
            {running ? "Running…" : "Run tests"}
          </button>
        </div>
        {!command.trim() && <p className="note">{CODE_NO_COMMAND}</p>}
        {confirming && (
          <div className="card confirm-window" role="group" aria-label={CODE_CONFIRM}>
            <p>{CODE_CONFIRM}</p>
            <pre className="code-command" aria-label="Command to run">
              {command.trim()}
            </pre>
            <div className="card-buttons">
              <button type="button" onClick={() => void confirmRun()} disabled={running}>
                Run tests
              </button>
              <button type="button" className="quiet" onClick={() => setConfirming(false)} disabled={running}>
                Cancel
              </button>
            </div>
          </div>
        )}
        {runNote && (
          <p className="note" role="status">
            {runNote}
          </p>
        )}
        {(log || running) && (
          <pre className="code-log" aria-label="Test output">
            {log || "Running…"}
          </pre>
        )}
        {needsConfirm && !confirming && command.trim() && !running && (
          <p className="note">The first run of this command will ask before it starts.</p>
        )}
      </div>
      {repo?.isRepo && <CodeShip key={activityKey} onShipped={() => void load()} />}
      {repo?.isRepo && <CodeDeploy onDeployed={() => setActivityKey((n) => n + 1)} />}
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
