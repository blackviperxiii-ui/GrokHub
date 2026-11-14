import {
  Activity,
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useEffectEvent,
  useRef,
  useState,
  type PointerEvent as ReactPointerEvent,
} from "react";
import {
  codeBuiltins,
  codeDiff,
  codeRepo,
  codeRunStop,
  codeSessionBind,
  codeSessionLead,
  codeSessionLeft,
  codeSessionResume,
  type CodeLeftOff,
  codeTestPlan,
  codeTestRun,
  onCodeRun,
  type CodeBuiltin,
  type CodeDiff,
  type CodeOverriddenBy,
  type CodeRepo,
} from "./api";
import { Agents } from "./Agents";
import { Chat, newConversationId, type Prefill } from "./Chat";
import { CodeDeploy } from "./CodeDeploy";
import { CodeProjects, folderName } from "./CodeProjects";
import type { View } from "./Palette";
import { CodeShip } from "./CodeShip";
import { CodeRevert } from "./CodeRevert";
import { inShell } from "./transport";
import { useCardFocus } from "./ui";

/** Right-column panes (former under-stack panels). */
export type CodeSideTab = "editor" | "tests" | "skills";

export const CODE_SIDE_TABS: { id: CodeSideTab; label: string }[] = [
  { id: "editor", label: "Editor" },
  { id: "tests", label: "Tests" },
  { id: "skills", label: "Skills" },
];

/** Remembered agent column width (% of the Code tab). Default ~62%. */
export const CODE_AGENT_WIDTH_KEY = "code.agentWidthPct";
export const CODE_AGENT_WIDTH_DEFAULT = 62;
export const CODE_AGENT_WIDTH_MIN = 48;
export const CODE_AGENT_WIDTH_MAX = 72;

export function readAgentWidthPct(): number {
  try {
    const raw = localStorage.getItem(CODE_AGENT_WIDTH_KEY);
    const n = raw == null ? NaN : Number(raw);
    if (!Number.isFinite(n)) return CODE_AGENT_WIDTH_DEFAULT;
    return Math.min(CODE_AGENT_WIDTH_MAX, Math.max(CODE_AGENT_WIDTH_MIN, n));
  } catch {
    return CODE_AGENT_WIDTH_DEFAULT;
  }
}

// CodeMirror and xterm load only when the tab first shows them.
const CodeEditor = lazy(() => import("./CodeEditor"));
const CodeTerminal = lazy(() => import("./CodeTerminal"));

export const CODE_NOT_REPO = "This project isn't a git repo yet. Open another project above, or ask the agent to start one.";
export const CODE_CLEAN = "No changes since the last commit.";
export const AGENT_TITLE = "Agent";
/** Under the agent window's title: the folder its session works in. */
export function worksIn(folder: string): string {
  return `Works in ${folderName(folder)}`;
}
export const RESUME = "Resume";

/** The one line under the agent title for a saved checkpoint. */
export function leftOffLine(left: CodeLeftOff): string {
  if (left.next) return `Next: ${left.next}`;
  if (left.blocker) return `Blocked: ${left.blocker}`;
  return `Goal: ${left.goal}`;
}
export const CODE_DESKTOP_ONLY = "The editor and terminal are only in the desktop app.";
export const CODE_REVIEW_PROMPT = "Review the uncommitted changes in this workspace: what changed, anything risky, and what to test before committing.";
export const CODE_BUILTINS = "Built-in skills";
export const CODE_TESTS = "Tests";
export const CODE_CONFIRM = "Run this test command?";
export const CODE_NO_COMMAND = "Type a test command to run.";
export const CODE_STOPPED = "Tests stopped.";

/** The label on a built-in row that a same-name skill currently shadows. */
export function overrideLabel(over: CodeOverriddenBy): string {
  return `overridden by ${over.scope} skill ${over.name}`;
}

interface Props {
  /** The Code tab is the one on screen. */
  shown: boolean;
  onOpenSettings: () => void;
  onSignIn?: () => void;
  onOpenView?: (view: View) => boolean | void;
  onOpenPalette?: () => void;
  grokSignIn?: boolean;
  /** The conversation the Chat tab has open. A session row for it goes
   * there: one conversation is never open in both places. */
  chatSession?: string;
  onShowChat?: () => void;
  /** The session in the agent window, each time it changes. */
  onSession?: (id: string) => void;
  /** Chats History deleted: the agent window leaves one it is showing. */
  deleted?: { id: string; ids?: string[]; n: number };
}

/** The Code tab: the project bar, the open project's sessions and its
 * workspace, with the agent window and its tasks beside them. */
export function Code({
  shown,
  onOpenSettings,
  onSignIn,
  onOpenView,
  onOpenPalette,
  grokSignIn,
  chatSession,
  onShowChat,
  onSession,
  deleted,
}: Props) {
  // Counts project switches: a new value starts the workspace below afresh in
  // the new folder, with nothing kept from the old one.
  const [switches, setSwitches] = useState(0);
  /** Counts commits and pushes made from the Tasks panel, so the workspace reads the repo again. */
  const [shipped, setShipped] = useState(0);
  /** What the agent window is told: a new session, with or without words in
   * its box, or a saved session to open. */
  const [prefill, setPrefill] = useState<Prefill>();
  const [open, setOpen] = useState<{ id: string; n: number }>();
  /** The session in the agent window, and the folder it works in. */
  const [session, setSession] = useState<string>();
  const [folder, setFolder] = useState("");
  /** The session started here, so it has a lead and its plan can be edited. */
  const [lead, setLead] = useState(false);
  const [left, setLeft] = useState<CodeLeftOff | null>(null);
  const [busy, setBusy] = useState(false);
  /** Counts replies ended, so the Sessions list reads again. */
  const [ended, setEnded] = useState(0);
  const sessionNow = useRef<string>(undefined);
  const busyNow = useRef(false);
  const appFolder = useRef<string>(undefined);
  const [sideTab, setSideTab] = useState<CodeSideTab>("editor");
  const [agentPct, setAgentPct] = useState(readAgentWidthPct);
  const tabRef = useRef<HTMLDivElement>(null);
  const drag = useRef<{ startX: number; startPct: number } | null>(null);

  /** A new session, with `text` waiting in its box. It has its folder
   * before the agent window shows it. */
  const startSession = useCallback((text: string) => {
    const id = newConversationId();
    const show = () => setPrefill((now) => ({ text, n: (now?.n ?? 0) + 1, conversationId: id }));
    codeSessionBind(id).then(show, show);
  }, []);

  /** A message to the lead, sent in the session the agent window shows. */
  const askLead = useCallback((text: string) => {
    const id = sessionNow.current;
    if (id) setPrefill((now) => ({ text, n: (now?.n ?? 0) + 1, send: true, conversationId: id }));
  }, []);

  /** A saved session. The Chat tab keeps the one it has open; any other
   * takes its folder first, so opening it is not the Chat tab's last chat. */
  function openSession(id: string) {
    if (id === chatSession) {
      onShowChat?.();
      return;
    }
    const show = () => setOpen((now) => ({ id, n: (now?.n ?? 0) + 1 }));
    codeSessionBind(id).then(show, show);
  }

  const onConversation = useCallback(
    (id: string) => {
      sessionNow.current = id;
      setSession(id);
      onSession?.(id);
      setFolder("");
      setLead(false);
      setLeft(null);
      codeSessionBind(id)
        .then((dir) => {
          if (sessionNow.current === id) setFolder(dir);
          return codeSessionLead(id);
        })
        .then(
          (is) => {
            if (sessionNow.current === id) setLead(is);
            return codeSessionLeft(id);
          },
          () => codeSessionLeft(id),
        )
        .then(
          (row) => {
            if (sessionNow.current === id) setLeft(row);
          },
          () => {},
        );
    },
    [onSession],
  );

  const resume = useCallback(() => {
    const id = sessionNow.current;
    if (!id || busyNow.current) return;
    codeSessionResume(id).then(
      (text) => {
        if (sessionNow.current !== id) return;
        setPrefill((now) => ({ text, n: (now?.n ?? 0) + 1, send: true, conversationId: id }));
      },
      () => {},
    );
  }, []);

  const onRan = useCallback(() => setEnded((n) => n + 1), []);

  const onBusy = useCallback((now: boolean) => {
    busyNow.current = now;
    setBusy(now);
    if (!now) setEnded((n) => n + 1);
  }, []);

  // A finished turn writes a new checkpoint. Read it again for the session
  // still on screen.
  useEffect(() => {
    const id = session;
    if (!id || ended === 0) return;
    codeSessionLeft(id).then(
      (row) => {
        if (sessionNow.current === id) setLeft(row);
      },
      () => {},
    );
  }, [ended, session]);

  // The app works in another folder than when this tab last looked (a switch
  // here, or a project bound in chat): the agent window starts a session
  // there. A reply still running keeps its session and its folder.
  const onFolder = useCallback(
    (next: string) => {
      const before = appFolder.current;
      appFolder.current = next;
      if (before !== undefined && before !== next && !busyNow.current) startSession("");
    },
    [startSession],
  );

  function onSplitDown(event: ReactPointerEvent<HTMLDivElement>) {
    if (event.button !== 0) return;
    event.preventDefault();
    drag.current = { startX: event.clientX, startPct: agentPct };
    event.currentTarget.setPointerCapture?.(event.pointerId);
  }

  function onSplitMove(event: ReactPointerEvent<HTMLDivElement>) {
    const start = drag.current;
    const root = tabRef.current;
    if (!start || !root) return;
    const width = root.getBoundingClientRect().width;
    if (width < 1) return;
    const deltaPct = ((event.clientX - start.startX) / width) * 100;
    const next = Math.min(CODE_AGENT_WIDTH_MAX, Math.max(CODE_AGENT_WIDTH_MIN, start.startPct + deltaPct));
    setAgentPct(next);
  }

  function onSplitUp(event: ReactPointerEvent<HTMLDivElement>) {
    if (!drag.current) return;
    drag.current = null;
    try {
      event.currentTarget.releasePointerCapture?.(event.pointerId);
    } catch {
      /* already released */
    }
    setAgentPct((pct) => {
      try {
        localStorage.setItem(CODE_AGENT_WIDTH_KEY, String(Math.round(pct * 10) / 10));
      } catch {
        /* private mode */
      }
      return pct;
    });
  }

  return (
    <div
      ref={tabRef}
      className="code-tab"
      hidden={!shown}
      style={{ ["--code-agent-pct" as string]: `${agentPct}%` }}
    >
      {/* Outside <Activity>, like Chat: hidden there, its listeners would
          stop and a reply landing on another tab would be lost. */}
      <aside className="code-agent" aria-label="Agent window">
        <header className="code-head">
          <h3>{AGENT_TITLE}</h3>
          {folder && (
            <span className="note" title={folder}>
              {worksIn(folder)}
            </span>
          )}
          {left && (
            <span className="note" title={left.goal}>
              {leftOffLine(left)}
            </span>
          )}
          {left && (
            <button type="button" className="quiet" disabled={busy} onClick={resume}>
              {RESUME}
            </button>
          )}
        </header>
        <Chat
          embedded
          lead={lead}
          active={shown}
          prefill={prefill}
          open={open}
          onConversation={onConversation}
          onBusy={onBusy}
          onRan={onRan}
          deleted={deleted}
          onOpenSettings={onOpenSettings}
          onSignIn={onSignIn}
          onOpenView={onOpenView}
          onOpenPalette={onOpenPalette}
          grokSignIn={grokSignIn}
        />
        <Agents
          conversationId={session}
          active={shown}
          live={lead}
          onAsk={lead ? askLead : undefined}
          onShipped={() => setShipped((n) => n + 1)}
        />
      </aside>
      <div
        className="code-split-handle"
        role="separator"
        aria-orientation="vertical"
        aria-label="Resize agent and editor"
        aria-valuenow={Math.round(agentPct)}
        aria-valuemin={CODE_AGENT_WIDTH_MIN}
        aria-valuemax={CODE_AGENT_WIDTH_MAX}
        tabIndex={0}
        onPointerDown={onSplitDown}
        onPointerMove={onSplitMove}
        onPointerUp={onSplitUp}
        onPointerCancel={onSplitUp}
        onKeyDown={(e) => {
          if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
          e.preventDefault();
          const step = e.key === "ArrowLeft" ? -2 : 2;
          setAgentPct((pct) => {
            const next = Math.min(CODE_AGENT_WIDTH_MAX, Math.max(CODE_AGENT_WIDTH_MIN, pct + step));
            try {
              localStorage.setItem(CODE_AGENT_WIDTH_KEY, String(next));
            } catch {
              /* private mode */
            }
            return next;
          });
        }}
      />
      <Activity mode={shown ? "visible" : "hidden"}>
        <section className="code code-side" aria-label="Code">
          <CodeProjects
            onSwitched={() => setSwitches((n) => n + 1)}
            onNewSession={() => startSession("")}
            onOpenSession={openSession}
            onFolder={onFolder}
            agentBusy={busy}
            openSession={session}
            refresh={ended}
          />
          <div className="code-side-tabs" role="tablist" aria-label="Code panels">
            {CODE_SIDE_TABS.map((tab) => (
              <button
                key={tab.id}
                type="button"
                role="tab"
                id={`code-side-tab-${tab.id}`}
                aria-selected={sideTab === tab.id}
                aria-controls={`code-side-panel-${tab.id}`}
                tabIndex={sideTab === tab.id ? 0 : -1}
                onClick={() => setSideTab(tab.id)}
              >
                {tab.label}
              </button>
            ))}
          </div>
          <div
            className="code-side-panel"
            role="tabpanel"
            id={`code-side-panel-${sideTab}`}
            aria-labelledby={`code-side-tab-${sideTab}`}
          >
            <CodeWorkspace key={switches} agentBusy={busy} onAsk={startSession} shipped={shipped} tab={sideTab} />
          </div>
        </section>
      </Activity>
    </div>
  );
}

/** The open project's repo, its changed files and one file's diff, an editor and a terminal, plus built-in skills and a test runner.
 * `shipped` counts commits and pushes made from the Tasks panel: each one reads the repo again. */
function CodeWorkspace({
  agentBusy,
  onAsk,
  shipped = 0,
  tab,
}: {
  agentBusy: boolean;
  onAsk: (prompt: string) => void;
  shipped?: number;
  tab: CodeSideTab;
}) {
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
  const [stopping, setStopping] = useState(false);
  // Counts Refresh presses, for the editor's file list.
  const [refreshes, setRefreshes] = useState(0);
  // True from a press on Stop until the next run starts.
  const stopAsked = useRef(false);
  const cancel = useCardFocus<HTMLButtonElement>(confirming);

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
    setRefreshes((n) => n + 1);
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

  // A commit or push from the Tasks panel changed the files list and the branch.
  useEffect(() => {
    if (shipped === 0) return;
    codeRepo().then(
      (next) => {
        setRepo(next);
        void rereadDiff(next.files);
      },
      (err) => setNote(String(err)),
    );
  }, [shipped, rereadDiff]);

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
    stopAsked.current = false;
    try {
      const result = await codeTestRun(cmd, confirmed);
      setLog(
        result.output + (result.cut ? "\n… cut here; the output is too long to show in full." : ""),
      );
      const stopped = stopAsked.current && result.exit !== 0;
      setRunNote(result.exit === 0 ? "Tests passed." : stopped ? CODE_STOPPED : `Tests exited with ${result.exit}.`);
      const plan = await codeTestPlan(cmd);
      setNeedsConfirm(plan.needsConfirm);
    } catch (err) {
      setRunNote(stopAsked.current ? CODE_STOPPED : String(err));
    } finally {
      setRunning(false);
    }
  }

  /** Stop the running tests and everything they started; the run then ends by itself. */
  async function stopRun() {
    setStopping(true);
    // True already when an earlier Stop of this run worked: a later one that fails does not undo it.
    const before = stopAsked.current;
    stopAsked.current = true;
    try {
      // False: nothing was running any more, or the kill failed. The run's own result stands.
      if (!(await codeRunStop())) stopAsked.current = before;
    } catch (err) {
      stopAsked.current = before;
      setRunNote(String(err));
    } finally {
      setStopping(false);
    }
  }

  const sync = repo && (repo.ahead || repo.behind) ? ` · ${repo.ahead} ahead, ${repo.behind} behind` : "";
  if (tab === "tests") {
    return (
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
          <button
            type="button"
            className={command.trim() ? "code-run cta" : "code-run"}
            onClick={() => void requestRun()}
            disabled={running || !command.trim()}
          >
            {running ? "Running…" : "Run tests"}
          </button>
          {running && (
            <button type="button" className="quiet" onClick={() => void stopRun()} disabled={stopping}>
              {stopping ? "Stopping…" : "Stop"}
            </button>
          )}
        </div>
        {!command.trim() && <p className="note">{CODE_NO_COMMAND}</p>}
        {confirming && (
          <div className="card confirm-window" role="group" aria-label={CODE_CONFIRM}>
            <p>{CODE_CONFIRM}</p>
            <pre className="code-command" aria-label="Command to run">
              {command.trim()}
            </pre>
            <div className="card-buttons">
              <button type="button" className="cta" onClick={() => void confirmRun()} disabled={running}>
                Run tests
              </button>
              <button ref={cancel} type="button" className="quiet" onClick={() => setConfirming(false)} disabled={running}>
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
          <pre className="code-log" tabIndex={0} role="log" aria-label="Test output">
            {log || "Running…"}
          </pre>
        )}
        {needsConfirm && !confirming && command.trim() && !running && (
          <p className="note">The first run of this command will ask before it starts.</p>
        )}
      </div>
    );
  }

  if (tab === "skills") {
    if (!builtins) return <p className="empty">Loading skills…</p>;
    if (builtins.length === 0) return <p className="empty">No built-in skills in this project.</p>;
    return (
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
                  {skill.overriddenBy && <span className="note">{overrideLabel(skill.overriddenBy)}</span>}
                </button>
              </li>
            ))}
          </ul>
          {openSkill && (
            <pre className="code-diff" tabIndex={0} role="region" aria-label={`Built-in skill ${openSkill.name}`}>
              {openSkill.body}
            </pre>
          )}
        </div>
      </div>
    );
  }

  return (
    <>
      <header className="code-head">
        <div>
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
            <button type="button" disabled={agentBusy} onClick={() => onAsk(CODE_REVIEW_PROMPT)}>
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
          <ul className="rows code-changed" aria-label="Changed files">
            {repo.files.map((f) => (
              <li key={f.path}>
                <button
                  type="button"
                  className={`code-file quiet ${f.status}`}
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
            <pre className="code-diff" tabIndex={0} role="region" aria-label={`Diff of ${diff.path}`}>
              {diff.text || "No text changes (binary or mode change)."}
              {diff.cut && "\n… cut here; the file's diff is too long to show in full."}
            </pre>
          )}
        </div>
      )}
      {repo !== null &&
        (inShell() ? (
          <Suspense fallback={<p className="note">Loading the editor and terminal…</p>}>
            <CodeEditor refresh={refreshes} />
            <CodeTerminal />
          </Suspense>
        ) : (
          <p className="note">{CODE_DESKTOP_ONLY}</p>
        ))}
      {repo?.isRepo && <CodeShip refreshKey={activityKey} onShipped={() => void load()} />}
      {repo?.isRepo && (
        <CodeRevert
          refreshKey={activityKey}
          onReverted={() => {
            setActivityKey((n) => n + 1);
            void load();
          }}
        />
      )}
      {repo?.isRepo && <CodeDeploy onDeployed={() => setActivityKey((n) => n + 1)} />}
    </>
  );
}
