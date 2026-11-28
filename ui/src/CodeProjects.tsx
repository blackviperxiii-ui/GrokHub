import { useEffect, useState, type FormEvent } from "react";
import {
  claudeSessionImport,
  claudeSessionPick,
  claudeSessionsList,
  codeProjectLinkRemote,
  codeProjectNew,
  codeProjectOpen,
  codeProjectPick,
  codeProjectSetBranch,
  codeProjects,
  listChats,
  type ChatRow,
  type ClaudeSessionRow,
  type CodeProject,
  type CodeProjects as ProjectList,
} from "./api";
import { whenLabel } from "./History";

export const PROJECTS_NONE_OPEN = "No project open";
export const PROJECTS_EMPTY = "No projects yet. Open a folder or start a new project.";
export const PROJECTS_NOT_LISTED = "This folder isn't in your project list yet.";
export const PROJECTS_MISSING = "folder not found";
export const SESSIONS_TITLE = "Sessions";
export const SESSIONS_EMPTY = "No sessions in this project yet.";
/** Sessions shown before Show all. */
export const SESSIONS_SHOWN = 6;
export const PROJECTS_AGENT_BUSY = "The agent is still replying. Stop it or wait, then switch.";
export const LINK_REPO = "Link repo…";
export const RULES_NONE = "No rule files found";
export const IMPORT_CLAUDE = "Import Claude session";
export const IMPORT_CLAUDE_EMPTY =
  "No Claude Code sessions match this workspace. Pick a session file, or run Claude Code in this folder first.";
export const IMPORT_CLAUDE_PICK = "Pick session file…";
export const IMPORT_CLAUDE_LIST = "Claude Code sessions";

/** One-line preview for a Claude session row (turns + when). */
export function claudeSessionPreview(row: ClaudeSessionRow, now: Date = new Date()): string {
  const turns = row.turnCount === 1 ? "1 turn" : `${row.turnCount} turns`;
  return `${turns} · ${whenLabel(row.updatedMs, now)}`;
}

/** The last part of a folder path, for a folder with no project row. */
export function folderName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

/** A chat bound to a Code project: its sessions and runners live in the
 * Editor (Projects / Helpers), not in Chat Hangout, History or Agents. */
export function isCodeSession(row: { project?: string | null; main?: boolean }): boolean {
  return !row.main && !!row.project;
}

/** The open project's saved chats, newest first. */
export function projectSessions(chats: ChatRow[], project: string): ChatRow[] {
  return chats.filter((c) => c.project === project).sort((a, b) => b.updated - a.updated);
}

/** One-line source / branch / sync for the open project. */
export function projectLinkLine(project: CodeProject): string {
  const parts: string[] = [];
  if (project.remoteUrl) {
    parts.push(project.remoteUrl);
  } else {
    parts.push("Local folder");
  }
  const branch = project.activeBranch || project.preferredBranch;
  if (branch) {
    parts.push(`on ${branch}`);
  }
  if (project.preferredBranch && project.activeBranch && project.preferredBranch !== project.activeBranch) {
    parts.push(`PR base ${project.preferredBranch}`);
  }
  if (project.sync && (project.sync.ahead || project.sync.behind)) {
    parts.push(`${project.sync.ahead} ahead, ${project.sync.behind} behind`);
  }
  return parts.join(" · ");
}

/** Short list of rule file names in force. */
export function projectRulesLine(project: CodeProject): string {
  const rules = project.rules ?? [];
  if (rules.length === 0) return RULES_NONE;
  const names = rules.map((r) => r.path).slice(0, 6);
  const more = rules.length > names.length ? ` (+${rules.length - names.length})` : "";
  return `Rules: ${names.join(", ")}${more}`;
}

interface Props {
  /** The app works in another folder now: the rest of the tab reads it again. */
  onSwitched: () => void;
  onNewSession: () => void;
  onOpenSession: (id: string) => void;
  /** Prefill a Code thread with imported Claude session context. */
  onImportClaude?: (context: string) => void;
  /** The folder the app works in, each time the bar reads it. */
  onFolder?: (folder: string) => void;
  /** The agent window's reply is running: the project and the session stay. */
  agentBusy?: boolean;
  /** The session in the agent window, marked in the list. */
  openSession?: string;
  /** A new value reads the open project and its sessions again (a reply
   * ended, or a command typed in the agent window ran). */
  refresh?: number;
}

/** The Code tab's project bar: the open project, the list to switch between,
 * Open folder and New project, and the open project's sessions. */
export function CodeProjects({
  onSwitched,
  onNewSession,
  onOpenSession,
  onImportClaude,
  onFolder,
  agentBusy = false,
  openSession,
  refresh = 0,
}: Props) {
  const [list, setList] = useState<ProjectList | null>(null);
  const [chats, setChats] = useState<ChatRow[]>([]);
  const [picking, setPicking] = useState(false);
  const [naming, setNaming] = useState(false);
  const [linking, setLinking] = useState(false);
  const [importing, setImporting] = useState(false);
  const [claudeRows, setClaudeRows] = useState<ClaudeSessionRow[] | null>(null);
  const [name, setName] = useState("");
  const [remoteUrl, setRemoteUrl] = useState("");
  const [branchOverride, setBranchOverride] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [all, setAll] = useState(false);

  // Runs each time the tab shows, so a project bound in chat and a session
  // started since are both here.
  useEffect(() => {
    codeProjects().then(setList, (err) => setNote(String(err)));
    listChats().then(setChats, () => {});
  }, [refresh]);
  const bound = list?.bound;
  useEffect(() => {
    if (bound !== undefined) onFolder?.(bound);
  }, [bound, onFolder]);

  /** Run one change of project; `null` from it means nothing was picked. */
  async function change(job: () => Promise<ProjectList | null>) {
    if (busy) return;
    if (agentBusy) {
      setNote(PROJECTS_AGENT_BUSY);
      return;
    }
    setBusy(true);
    setNote(null);
    try {
      const next = await job();
      if (!next) return;
      const moved = next.bound !== list?.bound;
      setList(next);
      setPicking(false);
      setNaming(false);
      setLinking(false);
      setName("");
      setRemoteUrl("");
      setBranchOverride("");
      setAll(false);
      listChats().then(setChats, () => {});
      if (moved) onSwitched();
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  function create(event: FormEvent) {
    event.preventDefault();
    const trimmed = name.trim();
    if (trimmed) void change(() => codeProjectNew(trimmed));
  }

  function linkRepo(event: FormEvent) {
    event.preventDefault();
    const url = remoteUrl.trim();
    if (!url) return;
    const branch = branchOverride.trim() || null;
    void change(() => codeProjectLinkRemote(url, branch));
  }

  function saveBranch(event: FormEvent) {
    event.preventDefault();
    const current = list?.projects.find((p) => p.current);
    if (!current) return;
    const branch = branchOverride.trim() || null;
    void change(() => codeProjectSetBranch(current.id, branch));
  }

  async function openClaudeImport() {
    if (busy || !onImportClaude) return;
    if (agentBusy) {
      setNote(PROJECTS_AGENT_BUSY);
      return;
    }
    const next = !importing;
    setImporting(next);
    setNaming(false);
    setLinking(false);
    setPicking(false);
    setNote(null);
    if (!next) {
      setClaudeRows(null);
      return;
    }
    setBusy(true);
    try {
      setClaudeRows(await claudeSessionsList());
    } catch (err) {
      setNote(String(err));
      setClaudeRows([]);
    } finally {
      setBusy(false);
    }
  }

  async function importClaudePath(path: string) {
    if (busy || !onImportClaude) return;
    if (agentBusy) {
      setNote(PROJECTS_AGENT_BUSY);
      return;
    }
    setBusy(true);
    setNote(null);
    try {
      const imported = await claudeSessionImport(path);
      onImportClaude(imported.context);
      setImporting(false);
      setClaudeRows(null);
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  async function pickClaudeFile() {
    if (busy || !onImportClaude) return;
    if (agentBusy) {
      setNote(PROJECTS_AGENT_BUSY);
      return;
    }
    setBusy(true);
    setNote(null);
    try {
      const imported = await claudeSessionPick();
      if (!imported) return;
      onImportClaude(imported.context);
      setImporting(false);
      setClaudeRows(null);
    } catch (err) {
      setNote(String(err));
    } finally {
      setBusy(false);
    }
  }

  const current = list?.projects.find((p) => p.current);
  const sessions = current ? projectSessions(chats, current.id) : [];
  const shown = all ? sessions : sessions.slice(0, SESSIONS_SHOWN);
  // With nothing open the list is the start of the tab.
  const showList = list !== null && (picking || !bound);
  return (
    <div className="code-projects">
      <header className="code-head">
        <div>
          <div className="card-title">{current?.name ?? (bound ? folderName(bound) : PROJECTS_NONE_OPEN)}</div>
          {bound && <div className="note">{bound}</div>}
          {current && (
            <>
              <div className="note">{projectLinkLine(current)}</div>
              <div className="note">{projectRulesLine(current)}</div>
            </>
          )}
        </div>
        <div className="card-buttons">
          {bound && list && list.projects.length > 0 && (
            <button type="button" className="quiet" aria-expanded={picking} onClick={() => setPicking((open) => !open)}>
              Switch project
            </button>
          )}
          <button type="button" className="quiet" onClick={() => void change(codeProjectPick)} disabled={busy}>
            Open folder…
          </button>
          <button
            type="button"
            className="quiet"
            aria-expanded={linking}
            onClick={() => {
              setLinking((open) => !open);
              setNaming(false);
            }}
          >
            {LINK_REPO}
          </button>
          <button
            type="button"
            className="quiet"
            aria-expanded={naming}
            onClick={() => {
              setNaming((open) => !open);
              setLinking(false);
            }}
          >
            New project
          </button>
        </div>
      </header>
      {note && (
        <p className="note" role="status">
          {note}
        </p>
      )}
      {naming && (
        <form className="code-test-row" onSubmit={create}>
          <label>
            Project name
            <input value={name} autoFocus autoComplete="off" disabled={busy} onChange={(e) => setName(e.target.value)} />
          </label>
          <button type="submit" disabled={busy || !name.trim()}>
            Create
          </button>
          <button type="button" className="quiet" onClick={() => setNaming(false)} disabled={busy}>
            Cancel
          </button>
        </form>
      )}
      {linking && (
        <form className="code-test-row" onSubmit={linkRepo}>
          <label>
            Repo URL
            <input
              value={remoteUrl}
              autoFocus
              autoComplete="off"
              placeholder="https://github.com/org/app.git"
              disabled={busy}
              onChange={(e) => setRemoteUrl(e.target.value)}
            />
          </label>
          <label>
            Preferred branch
            <input
              value={branchOverride}
              autoComplete="off"
              placeholder="beta (optional)"
              disabled={busy}
              onChange={(e) => setBranchOverride(e.target.value)}
            />
          </label>
          <button type="submit" disabled={busy || !remoteUrl.trim()}>
            Link
          </button>
          <button type="button" className="quiet" onClick={() => setLinking(false)} disabled={busy}>
            Cancel
          </button>
        </form>
      )}
      {current && !linking && !naming && (
        <form className="code-test-row" onSubmit={saveBranch}>
          <label>
            Working branch
            <input
              value={branchOverride}
              autoComplete="off"
              placeholder={current.preferredBranch ?? "beta"}
              disabled={busy}
              onChange={(e) => setBranchOverride(e.target.value)}
            />
          </label>
          <button type="submit" disabled={busy}>
            Set branch
          </button>
        </form>
      )}
      {showList &&
        (list.projects.length === 0 ? (
          <p className="empty">{PROJECTS_EMPTY}</p>
        ) : (
          <ul className="rows code-project-list" aria-label="Projects">
            {list.projects.map((p) => (
              <li key={p.id}>
                <button
                  type="button"
                  className="code-skill quiet"
                  aria-current={p.current ? "true" : undefined}
                  disabled={busy || p.current}
                  onClick={() => void change(() => codeProjectOpen(p.path))}
                >
                  <span className="card-title">{p.name}</span>
                  <span className="note">
                    {p.path}
                    {p.current ? " · open" : p.missing ? ` · ${PROJECTS_MISSING}` : ""}
                  </span>
                </button>
              </li>
            ))}
          </ul>
        ))}
      {bound && list && !current && (
        <p className="note">
          {PROJECTS_NOT_LISTED}{" "}
          <button type="button" className="quiet" onClick={() => void change(() => codeProjectOpen(bound))} disabled={busy}>
            Add to projects
          </button>
        </p>
      )}
      {current && (
        <div className="code-sessions">
          <div className="code-head">
            <h3>{SESSIONS_TITLE}</h3>
            <div className="card-buttons">
              {onImportClaude && (
                <button
                  type="button"
                  className="quiet"
                  aria-expanded={importing}
                  disabled={agentBusy || busy}
                  onClick={() => void openClaudeImport()}
                >
                  {IMPORT_CLAUDE}
                </button>
              )}
              <button type="button" onClick={onNewSession} disabled={agentBusy}>
                New session
              </button>
            </div>
          </div>
          {importing && (
            <div className="code-claude-import" aria-label={IMPORT_CLAUDE_LIST}>
              <div className="code-head">
                <h4>{IMPORT_CLAUDE_LIST}</h4>
                <button type="button" className="quiet" disabled={busy} onClick={() => void pickClaudeFile()}>
                  {IMPORT_CLAUDE_PICK}
                </button>
              </div>
              {claudeRows === null ? (
                <p className="note">Loading…</p>
              ) : claudeRows.length === 0 ? (
                <p className="note">{IMPORT_CLAUDE_EMPTY}</p>
              ) : (
                <ul className="rows code-project-list" aria-label={IMPORT_CLAUDE_LIST}>
                  {claudeRows.map((row) => (
                    <li key={row.path}>
                      <button
                        type="button"
                        className="code-skill quiet"
                        disabled={busy || agentBusy}
                        onClick={() => void importClaudePath(row.path)}
                      >
                        <span className="card-title">{row.title}</span>
                        <span className="note">{claudeSessionPreview(row)}</span>
                      </button>
                    </li>
                  ))}
                </ul>
              )}
            </div>
          )}
          {sessions.length === 0 ? (
            <p className="note">{SESSIONS_EMPTY}</p>
          ) : (
            <ul className="rows code-project-list" aria-label={SESSIONS_TITLE}>
              {shown.map((c) => (
                <li key={c.id}>
                  <button
                    type="button"
                    className="code-skill quiet"
                    aria-current={c.id === openSession ? "true" : undefined}
                    disabled={agentBusy && c.id !== openSession}
                    onClick={() => onOpenSession(c.id)}
                  >
                    <span className="card-title">{c.title || "Untitled chat"}</span>
                    <span className="note">{whenLabel(c.updated)}</span>
                  </button>
                </li>
              ))}
            </ul>
          )}
          {sessions.length > SESSIONS_SHOWN && (
            <button type="button" className="quiet" aria-expanded={all} onClick={() => setAll((open) => !open)}>
              {all ? "Show fewer" : `Show all ${sessions.length}`}
            </button>
          )}
        </div>
      )}
    </div>
  );
}
