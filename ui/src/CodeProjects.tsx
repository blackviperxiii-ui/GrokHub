import { useEffect, useState, type FormEvent } from "react";
import {
  codeProjectNew,
  codeProjectOpen,
  codeProjectPick,
  codeProjects,
  listChats,
  type ChatRow,
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

/** The last part of a folder path, for a folder with no project row. */
export function folderName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

/** The open project's saved chats, newest first. */
export function projectSessions(chats: ChatRow[], project: string): ChatRow[] {
  return chats.filter((c) => c.project === project).sort((a, b) => b.updated - a.updated);
}

interface Props {
  /** The app works in another folder now: the rest of the tab reads it again. */
  onSwitched: () => void;
  onNewSession: () => void;
  onOpenSession: (id: string) => void;
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
  onFolder,
  agentBusy = false,
  openSession,
  refresh = 0,
}: Props) {
  const [list, setList] = useState<ProjectList | null>(null);
  const [chats, setChats] = useState<ChatRow[]>([]);
  const [picking, setPicking] = useState(false);
  const [naming, setNaming] = useState(false);
  const [name, setName] = useState("");
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
      setName("");
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
          <button type="button" className="quiet" aria-expanded={naming} onClick={() => setNaming((open) => !open)}>
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
            <button type="button" onClick={onNewSession} disabled={agentBusy}>
              New session
            </button>
          </div>
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
