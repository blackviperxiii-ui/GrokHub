import { Activity, Component, lazy, Suspense, useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import {
  CORE_ONLY,
  getCapabilities,
  getSettings,
  getWorkboard,
  haltDesktop,
  listChats,
  onAutomationRan,
  onChatEvent,
  onHalt,
  onLocalEngineEvent,
  onUpdateAvailable,
  shareSchedulerClock,
  WEB,
  workboardSweepUndo,
  type BoardSwept,
  type Capabilities,
} from "./api";
import {
  cardsByChat,
  chatActivity,
  pruneActivity,
  queuedItems,
  queueSpot,
  routineFinished,
  routineStarted,
  type ActivityItem,
} from "./activity";
import { ActivityDock } from "./ActivityDock";
import { Automations, resultLine, type RunNotice } from "./Automations";
import { NoticeBell } from "./NoticeBell";
import { onAutoResumeStopped } from "./autoResume";
import {
  chatFrom,
  clearNotices,
  markRead,
  NO_NOTICES,
  pushNotice,
  spendAction,
  type Notice,
  type NoticeAction,
  type NoticeKind,
  type Notices,
} from "./notices";
import { Chat, newConversationId, type Prefill, type QueueAct } from "./Chat";
import { useChipRow } from "./Chips";
import { RecordIndicator } from "./RecordIndicator";
import { HALT_DONE, HALT_FAILED, HaltButton, HaltNote, haltShown, isHaltKey } from "./Halt";
import { NO_SOURCES, Palette, VIEW_LABELS, hasPage, moreViews, type PaletteAction, type View } from "./Palette";
import { COMPOSER_SLASH, shellKeys, type CommandRunners } from "./paletteCommands";
import { Shortcuts } from "./Shortcuts";
import type { Section } from "./sections";
import { useOverscroll } from "./rubberBand";
import { useHidingBar } from "./tabBar";
import { applyTheme } from "./theme";
import { inShell } from "./transport";
import { LOGO_MARK_URL, PRODUCT_NAME } from "./brand";
import { Rail } from "./Rail";
import { ChatSide } from "./ChatSide";
import { StatusPills } from "./StatusPills";
import { ChatTabMenu, ClearChatDialog } from "./ClearChat";
import { AutoNote, FirstRun, useAutoSetup } from "./AutoSetup";
import { OnboardStage } from "./GetToKnowYou";

// Chat and Automations stay mounted, so they load up front. The other screens
// load the first time they open, and again in the background once the app is
// idle so that first click does not wait on the network or disk.
const loaders = {
  agents: () => import("./Agents").then((m) => ({ default: m.Agents })),
  code: () => import("./Code").then((m) => ({ default: m.Code })),
  command: () => import("./CommandBox").then((m) => ({ default: m.CommandBox })),
  connectors: () => import("./Connectors").then((m) => ({ default: m.Connectors })),
  imagine: () => import("./Imagine").then((m) => ({ default: m.Imagine })),
  devices: () => import("./Devices").then((m) => ({ default: m.Devices })),
  journal: () => import("./Journal").then((m) => ({ default: m.Journal })),
  workboard: () => import("./Workboard").then((m) => ({ default: m.Workboard })),
  history: () => import("./History").then((m) => ({ default: m.History })),
  home: () => import("./Home").then((m) => ({ default: m.Home })),
  library: () => import("./Library").then((m) => ({ default: m.Library })),
  memory: () => import("./Memory").then((m) => ({ default: m.Memory })),
  mods: () => import("./Mods").then((m) => ({ default: m.Mods })),
  objectives: () => import("./Objectives").then((m) => ({ default: m.Objectives })),
  skills: () => import("./Skills").then((m) => ({ default: m.Skills })),
  pulse: () => import("./Pulse").then((m) => ({ default: m.Pulse })),
  settings: () => import("./Settings").then((m) => ({ default: m.Settings })),
  updates: () => import("./UpdatesDrivers").then((m) => ({ default: m.UpdatesDrivers })),
};
const Agents = lazy(loaders.agents);
const Connectors = lazy(loaders.connectors);
const Imagine = lazy(loaders.imagine);
const Devices = lazy(loaders.devices);
const History = lazy(loaders.history);
const Home = lazy(loaders.home);
const Journal = lazy(loaders.journal);
const Code = lazy(loaders.code);
const CommandBox = lazy(loaders.command);
const Library = lazy(loaders.library);
const Memory = lazy(loaders.memory);
const Mods = lazy(loaders.mods);
const Objectives = lazy(loaders.objectives);
const Pulse = lazy(loaders.pulse);
const Skills = lazy(loaders.skills);
const Settings = lazy(loaders.settings);
const Workboard = lazy(loaders.workboard);
const UpdatesDrivers = lazy(loaders.updates);

/** How often the window tells the scheduler its clock. */
const CLOCK_SHARE_MS = 5 * 60 * 1000;

function preloadViews() {
  for (const load of Object.values(loaders)) void load().catch(() => {});
}

class PageBoundary extends Component<{ shown?: boolean; children: ReactNode }, { error: Error | null }> {
  state: { error: Error | null } = { error: null };

  static getDerivedStateFromError(error: Error): { error: Error } {
    return { error };
  }

  render() {
    if (this.state.error) {
      if (this.props.shown === false) return null;
      return (
        <div role="alert" className="card">
          <p>This page hit a snag: {this.state.error.message || "Unexpected error"}</p>
          <button type="button" className="quiet" onClick={() => this.setState({ error: null })}>
            Try again
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

/** A page that stays mounted once it has opened. Hidden, it keeps its state
 * (a half-typed form stays) and its effects stop; they run again when it
 * shows, so its lists reload. */
function Page({ shown, children }: { shown: boolean; children: ReactNode }) {
  return (
    <Activity mode={shown ? "visible" : "hidden"}>
      <PageBoundary>
        <Suspense fallback={null}>{children}</Suspense>
      </PageBoundary>
    </Activity>
  );
}


/** How much of a run's reply its notification shows; Automations has the rest. */
export const NOTICE_CHARS = 200;

/** "your chat" -> "Your chat"; a quoted name stays as it is. */
function capital(name: string): string {
  return name.charAt(0).toUpperCase() + name.slice(1);
}

export function runNoticeText(run: RunNotice): string {
  const text = run.text.length > NOTICE_CHARS ? `${run.text.slice(0, NOTICE_CHARS).trimEnd()}…` : run.text;
  return `${run.name} ${run.ok ? "finished" : "failed"}${text ? `: ${text}` : ""}`;
}

export function runNoticeAction(run: RunNotice): NoticeAction {
  return run.ok
    ? { kind: "draft", label: "Ask about it", text: `About the "${run.name}" automation run: ` }
    : {
        kind: "draft",
        label: "Find the cause",
        text: `The "${run.name}" automation failed: ${run.text}. What went wrong and how do I fix it?`,
      };
}

export function App() {
  const [view, setView] = useState<View>("chat");
  const [prefill, setPrefill] = useState<Prefill>();
  // A browser is the web app from its first render: it must not run a
  // desktop-only step (the last chat, say) while its capabilities load.
  const [caps, setCaps] = useState<Capabilities>(() => (inShell() ? CORE_ONLY : WEB));
  // The palette: everything (Ctrl+K, Command), or the rail's More, pages only.
  const [palette, setPalette] = useState<false | "all" | "more">(false);
  const [sheet, setSheet] = useState(false);
  const paletteChips = useChipRow({ surface: "palette", active: palette === "all" });
  const [signedOut, setSignedOut] = useState(false);
  const [goTo, setGoTo] = useState<{ section: Section; n: number }>();
  const [notices, setNotices] = useState<Notices>(NO_NOTICES);
  const [open, setOpen] = useState<{ id: string; n: number }>();
  /** The chat History deleted last, and every chat it has deleted: Delete all
   * reports its chats in one tick, so one id would lose all but the last. */
  const [deleted, setDeleted] = useState<{ id: string; ids: string[]; n: number }>();
  /** The Chat tab's right-click menu, at the pointer; then its confirm; then the clear. */
  const [tabMenu, setTabMenu] = useState<{ x: number; y: number } | null>(null);
  const [askClear, setAskClear] = useState(false);
  const [clear, setClear] = useState<{ n: number }>();
  const [activity, setActivity] = useState<Record<string, ActivityItem>>({});
  // Messages lined up behind the open chat's reply, as the chat reports them.
  const [queued, setQueued] = useState<{ conversationId: string; texts: string[] }>();
  const [queueAct, setQueueAct] = useState<QueueAct>();
  const onQueue = useCallback((conversationId: string, texts: string[]) => {
    setQueued((now) =>
      now && now.conversationId === conversationId && now.texts.join("\u0000") === texts.join("\u0000")
        ? now
        : { conversationId, texts },
    );
  }, []);
  // Which work card each chat works, from the Workboard's last load.
  const [cardChats, setCardChats] = useState<Record<string, string>>({});
  const [openCard, setOpenCard] = useState<{ id: string; n: number }>();
  const [desktopOn, setDesktopOn] = useState(false);
  const [haltNote, setHaltNote] = useState<string>();
  const auto = useAutoSetup();
  // Halt is the computer's own: phones and the web app have no desktop control.
  const canHalt = caps.desktopControl && !caps.remote;
  const busy = Object.values(activity).some((item) => item.status === "running" || item.status === "waiting");
  // The open chat for the Agents page: the conversation the window last heard from.
  const [openChat, setOpenChat] = useState<string>();
  // The conversation each pane has open: the Chat tab's, and the Code tab's
  // agent window's. One conversation is only ever open in one of them.
  const [chatSession, setChatSession] = useState<string>();
  const [agentSession, setAgentSession] = useState<string>();
  const agentNow = useRef<string>(undefined);
  const onAgentSession = useCallback((id: string) => {
    agentNow.current = id;
    setAgentSession(id);
  }, []);
  // The pages opened so far. Each mounts the first time it opens, not before.
  const [opened, setOpened] = useState<ReadonlySet<View>>(() => new Set(["chat"]));
  if (!opened.has(view)) setOpened(new Set(opened).add(view));
  // A screen recording or an audio check is running: the bar holding its
  // clock and Stop stays on screen in a narrow window too.
  const [recLive, setRecLive] = useState(false);
  const barAway = useHidingBar(palette !== false || (canHalt && recLive));
  // Closing Settings goes back to the screen that was open before it, and its
  // Back button names that screen.
  const [before, setBefore] = useState<View>("chat");
  if (view !== "settings" && before !== view) setBefore(view);
  useOverscroll();
  const notify = useCallback((kind: NoticeKind, text: string, action?: NoticeAction, from?: string) => {
    setNotices((now) => pushNotice(now, kind, text, Date.now(), action, from));
  }, []);
  /** Something happened in a chat: the entry names the chat and opens it. */
  const notifyChat = useCallback(
    (kind: NoticeKind, conversationId: string, words: (name: string) => string, label: string) => {
      // A side answer (`<chat>~side-<n>`) belongs to its chat.
      const id = conversationId.split("~side-")[0];
      const post = (title?: string) => {
        const name = title?.trim();
        notify(kind, words(name ? `"${name}"` : "your chat"), { kind: "chat", label, id }, chatFrom(name));
      };
      listChats().then(
        (rows) => post(rows.find((row) => row.id === id)?.title),
        () => post(),
      );
    },
    [notify],
  );
  /** An automation run ended: its result, with a one-tap follow-up. */
  const notifyRun = useCallback(
    (run: RunNotice) =>
      notify(run.ok ? "success" : "error", runNoticeText(run), runNoticeAction(run), "Automations"),
    [notify],
  );
  // A Workboard tidy, read at start and whenever the Workboard loads.
  const swept = useRef<number>(undefined);
  const noteSweep = useCallback(
    (sweep: BoardSwept | null) => {
      if (!sweep || swept.current === sweep.at) return;
      swept.current = sweep.at;
      notify("info", sweep.text, { kind: "undoSweep", label: "Undo" }, "Workboard");
    },
    [notify],
  );
  useEffect(() => {
    const idle = window.requestIdleCallback;
    if (idle) {
      const handle = idle(preloadViews, { timeout: 3000 });
      return () => window.cancelIdleCallback(handle);
    }
    const timer = window.setTimeout(preloadViews, 1500);
    return () => window.clearTimeout(timer);
  }, []);
  useEffect(() => {
    getCapabilities()
      .catch(() => CORE_ONLY)
      .then(setCaps);
    getSettings().then(
      (s) => applyTheme(s.theme),
      () => {},
    );
    // A local read, so a chat's dock item can open its work card.
    getWorkboard().then(
      (board) => {
        setCardChats(cardsByChat(board));
        noteSweep(board.swept);
      },
      () => {},
    );
  }, [noteSweep]);
  // Settings may have turned desktop control on or off; read it again when
  // the view changes (leaving Settings included).
  useEffect(() => {
    if (!canHalt) return;
    getSettings().then(
      (s) => setDesktopOn(s.desktopControl),
      () => {},
    );
  }, [canHalt, view]);
  // The tray's Halt and Super+Shift+Esc land here too.
  useEffect(() => {
    if (!canHalt) return;
    let stop: (() => void) | undefined;
    let dropped = false;
    void onHalt(() => setHaltNote(HALT_DONE)).then((unlisten) => {
      if (dropped) unlisten();
      else stop = unlisten;
    });
    return () => {
      dropped = true;
      stop?.();
    };
  }, [canHalt]);
  // The palette's Sign in row follows the key and the Grok sign-in, which
  // Settings may have added.
  useEffect(() => {
    if (palette === "all") getSettings().then((s) => setSignedOut(!s.hasApiKey && !s.signedIn), () => {});
  }, [palette]);
  // The Activity Dock follows every chat turn, whichever view is open.
  useEffect(() => {
    let stop: (() => void) | undefined;
    let dropped = false;
    void onChatEvent((envelope) => {
      // The agent window's session has its own Tasks panel on the Code tab.
      if (envelope.conversationId.split("~side-")[0] !== agentNow.current) setOpenChat(envelope.conversationId);
      // While the window is in the background, what a reply did lands in the
      // bell too, so it is there on return (the OS notification is the echo).
      if (!document.hasFocus()) {
        const { event } = envelope;
        const id = envelope.conversationId;
        if (event.kind === "done" && event.stop !== "halted" && event.stop !== "cancelled")
          notifyChat("success", id, (name) => `Reply finished in ${name}`, "Open chat");
        else if (event.kind === "permission")
          notifyChat("info", id, (name) => `${capital(name)} needs your OK: ${event.title}`, "Review");
        else if (event.kind === "error")
          notifyChat("error", id, (name) => `Reply failed in ${name}: ${event.message}`, "Open chat");
      }
      setActivity((now) => {
        const key = `chat:${envelope.conversationId}`;
        const next = chatActivity(now[key], envelope, Date.now());
        if (next === now[key]) return now;
        const copy = pruneActivity({ ...now }, Date.now());
        if (next) copy[key] = next;
        else delete copy[key];
        return copy;
      });
    }).then((unlisten) => {
      if (dropped) unlisten();
      else stop = unlisten;
    });
    return () => {
      dropped = true;
      stop?.();
    };
  }, [notifyChat]);
  // A new update, the local engine finishing or failing, and a Workboard
  // tidy land in the bell, whichever page is open.
  useEffect(() => {
    const stops: (() => void)[] = [];
    let dropped = false;
    const keep = (unlisten: () => void) => {
      if (dropped) unlisten();
      else stops.push(unlisten);
    };
    void onUpdateAvailable((update) => {
      if (update.available)
        notify(
          "info",
          `Version ${update.available} is ready to install.`,
          {
            kind: "settings",
            label: "Open About",
            section: "about",
          },
          "Settings › About",
        );
    }).then(keep);
    let downloading = false;
    void onLocalEngineEvent((event) => {
      if (event.error) {
        downloading = false;
        notify(
          "error",
          `Local AI: ${event.error}`,
          { kind: "settings", label: "Open Local AI", section: "localAi" },
          "Settings › Local AI",
        );
      } else if (event.state === "downloading") {
        downloading = true;
      } else if (event.state === "ready" && downloading) {
        downloading = false;
        notify(
          "success",
          event.message ?? "Local AI is ready.",
          { kind: "settings", label: "Open Local AI", section: "localAi" },
          "Settings › Local AI",
        );
      }
    }).then(keep);
    return () => {
      dropped = true;
      for (const stop of stops) stop();
    };
  }, [notify]);
  const halt = useCallback(() => {
    haltDesktop().then(
      () => setHaltNote(HALT_DONE),
      (err) => setHaltNote(`${HALT_FAILED} ${String(err)}`),
    );
  }, []);
  // The shell's scheduler fires jobs at local times, so it needs this
  // window's clock; again every few minutes for daylight saving.
  useEffect(() => {
    const share = () => void shareSchedulerClock().catch(() => {});
    share();
    const timer = window.setInterval(share, CLOCK_SHARE_MS);
    return () => window.clearInterval(timer);
  }, []);
  // An auto-resume stopped for the day: the card waits in Blocked, and the
  // bell says so.
  useEffect(() => {
    let stop: (() => void) | undefined;
    let dropped = false;
    void onAutoResumeStopped(({ text }) => notify("info", text, undefined, "Workboard")).then((unlisten) => {
      if (dropped) unlisten();
      else stop = unlisten;
    });
    return () => {
      dropped = true;
      stop?.();
    };
  }, [notify]);
  // A scheduled run ended: the bell keeps the result and the Activity Dock
  // shows it for a while.
  useEffect(() => {
    let stop: (() => void) | undefined;
    let dropped = false;
    void onAutomationRan(({ id, name, result }) => {
      const run: RunNotice = { id, name, ok: !result.error, text: resultLine(result) };
      notifyRun(run);
      setActivity((now) => {
        const item = routineStarted(id, name, Date.now());
        return {
          ...pruneActivity(now, Date.now()),
          [`run:${id}`]: routineFinished(item, run.ok, run.text, Date.now()),
        };
      });
    }).then((unlisten) => {
      if (dropped) unlisten();
      else stop = unlisten;
    });
    return () => {
      dropped = true;
      stop?.();
    };
  }, [notifyRun]);
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (canHalt && isHaltKey(event)) {
        event.preventDefault();
        halt();
        return;
      }
      if (!(event.ctrlKey || event.metaKey) || event.altKey) return;
      const key = event.key.toLowerCase();
      if (key === "k") {
        event.preventDefault();
        setPalette((open) => (open ? false : "all"));
      } else if (key === "n" || key === "," || key === "/") {
        event.preventDefault();
        setPalette(false);
        if (key === "n") newChat();
        else if (key === ",") setView("settings");
        else setSheet((open) => !open);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [canHalt, halt]);

  /** A page a tile, a chip or the chat asks for: opened only if this app has
   * it (`hasPage`). Whether it opened, so `/hub` can say when it did not. */
  function openView(next: View): boolean {
    const here = hasPage(caps, next);
    if (here) setView(next);
    return here;
  }

  /** A task the person typed and ran on another page: Chat opens and sends it. */
  function askNow(text: string) {
    setPrefill((now) => ({ text, n: (now?.n ?? 0) + 1, send: true }));
    setView("chat");
  }

  /** Queue → Run: Chat opens in the job's chat and sends its prompt as a
   * normal turn (the cabin's `start_queued_job`). */
  function runQueued(prompt: string, threadId: string) {
    setPrefill((now) => ({ text: prompt, n: (now?.n ?? 0) + 1, send: true, ...(threadId ? { conversationId: threadId } : {}) }));
    setView("chat");
  }

  /** Text for the chat box; with `conversationId`, in that chat (a Pulse card's own). */
  function ask(text: string, conversationId?: string) {
    setPrefill((now) => ({ text, n: (now?.n ?? 0) + 1, ...(conversationId ? { conversationId } : {}) }));
    setView("chat");
  }

  /** A fresh conversation, as the chat's own New chat button starts. */
  function newChat() {
    setPrefill((now) => ({ text: "", n: (now?.n ?? 0) + 1, conversationId: newConversationId() }));
    setView("chat");
  }

  // The palette's commands this shell can run; the others have no row yet.
  const runners: CommandRunners = {
    "new-chat": newChat,
    halt,
    "sign-in": () => run({ kind: "settings", section: "account" }),
    shortcuts: () => setSheet(true),
  };

  /** A saved chat, opened in Chat as History opens one. The web app has no
   * History, so there it switches to the browser's own chat. */
  function openSaved(id: string) {
    // The agent window has it open: it shows there, cards and all.
    if (id === agentSession && opened.has("code")) {
      setView("code");
      return;
    }
    if (caps.remote) setPrefill((now) => ({ text: "", n: (now?.n ?? 0) + 1, conversationId: id }));
    else setOpen((now) => ({ id, n: (now?.n ?? 0) + 1 }));
    setView("chat");
  }

  function run(action: PaletteAction) {
    if (action.kind === "view") setView(action.view);
    else if (action.kind === "ask") ask(action.text);
    else if (action.kind === "command") runners[action.id]?.();
    else if (action.kind === "chat") {
      setPrefill((now) => ({ text: "", n: (now?.n ?? 0) + 1, conversationId: action.id }));
      setView("chat");
    } else {
      setGoTo((now) => ({ section: action.section, n: (now?.n ?? 0) + 1 }));
      setView("settings");
    }
  }
  function noticeAction(picked: Notice) {
    const action = picked.action;
    if (!action) return;
    setNotices((now) => spendAction(now, picked.id));
    if (action.kind === "draft") ask(action.text);
    else if (action.kind === "chat") {
      if (action.id) openSaved(action.id);
      else setView("chat");
    }
    else if (action.kind === "automations") setView("automations");
    else if (action.kind === "settings") run({ kind: "settings", section: action.section });
    else
      workboardSweepUndo().then(
        () => notify("success", "Put the archived cards back in Todo.", undefined, "Workboard"),
        (err) => notify("error", `Undo didn't go through: ${String(err)}`, undefined, "Workboard"),
      );
  }
  return (
    <div className={barAway ? "app bar-away" : "app"}>
      <header className="topbar">
        <div className="brand-mark">
          <img src={LOGO_MARK_URL} alt="" width={22} height={22} />
          <span>{PRODUCT_NAME}</span>
        </div>
        <div className="topbar-fill" />
        <StatusPills
          remote={Boolean(caps.remote)}
          refresh={`${view === "settings"}:${busy}`}
          onSettings={(section) => run({ kind: "settings", section })}
        />
        {canHalt && haltShown(desktopOn, busy) && <HaltButton onHalt={halt} />}
        <ActivityDock
          items={[
            ...Object.values(activity),
            ...(queued ? queuedItems(queued.conversationId, queued.texts) : []),
          ]}
          canSteer={(item) => {
            const spot = queueSpot(item.key);
            return Boolean(spot && activity[`chat:${spot.conversationId}`]?.status === "running");
          }}
          onSteer={(item) => {
            const spot = queueSpot(item.key);
            if (spot) setQueueAct((now) => ({ op: "steer", index: spot.index, n: (now?.n ?? 0) + 1 }));
          }}
          onRemove={(item) => {
            const spot = queueSpot(item.key);
            if (spot) setQueueAct((now) => ({ op: "remove", index: spot.index, n: (now?.n ?? 0) + 1 }));
          }}
          onOpenChat={() => setView("chat")}
          onOpenAutomations={() => setView("automations")}
          cardFor={(chat) => cardChats[chat]}
          onOpenCard={(id) => {
            setOpenCard((now) => ({ id, n: (now?.n ?? 0) + 1 }));
            setView("workboard");
          }}
        />
        {canHalt && (
          <RecordIndicator
            onEvent={(event) =>
              notify(event.ok ? "success" : "error", event.text, undefined, event.kind === "audio" ? "Audio check" : "Screen recording")
            }
            onLive={setRecLive}
          />
        )}
        <NoticeBell
          notices={notices}
          onOpen={() => setNotices(markRead)}
          onClear={() => setNotices(clearNotices)}
          onAction={noticeAction}
        />
        <button
          type="button"
          className="palette-open"
          title="Command palette (Ctrl+K)"
          onClick={() => setPalette("all")}
        >
          Command
        </button>
      </header>
      <div className="app-body">
        <Rail
          caps={caps}
          view={view}
          moreOpen={palette === "more"}
          onOpen={setView}
          onMore={() => setPalette((open) => (open === "more" ? false : "more"))}
          onChatMenu={setTabMenu}
        />
        <main className="views">
          {haltNote && <HaltNote text={haltNote} onDismiss={() => setHaltNote(undefined)} />}
          {caps.autoSetup && auto.reply?.firstRun && <FirstRun reply={auto.reply} act={auto.act} />}
          {caps.autoSetup && <OnboardStage after={auto.reply !== null && !auto.reply.firstRun} />}
          {caps.autoSetup && auto.note && !auto.reply?.firstRun && (
            <AutoNote note={auto.note} act={auto.act} onClose={auto.closeNote} />
          )}
          {palette === "all" && (
            <Palette
              caps={caps}
              sources={{
                ...NO_SOURCES,
                runners,
                signedOut,
                slash: COMPOSER_SLASH,
                chips: paletteChips.chips,
                onChip: paletteChips.picked,
              }}
              onPick={run}
              onClose={() => setPalette(false)}
            />
          )}
          {palette === "more" && (
            <Palette caps={caps} only={moreViews(caps)} onPick={run} onClose={() => setPalette(false)} />
          )}
          {tabMenu && <ChatTabMenu at={tabMenu} onClear={() => setAskClear(true)} onClose={() => setTabMenu(null)} />}
          {sheet && <Shortcuts keys={shellKeys(caps, runners)} onClose={() => setSheet(false)} />}
          {/* Chat and Automations stay mounted so a reply or a Run now in flight
              is not lost when another tab opens. */}
          {askClear && (
            <ClearChatDialog
              onConfirm={() => {
                setAskClear(false);
                setView("chat");
                setClear((now) => ({ n: (now?.n ?? 0) + 1 }));
              }}
              onClose={() => setAskClear(false)}
            />
          )}
          <div hidden={view !== "chat"}>
            <Chat
              onOpenSettings={() => setView("settings")}
              onSignIn={() => run({ kind: "settings", section: "account" })}
              onOpenView={openView}
              onOpenSection={(section) => run({ kind: "settings", section })}
              onOpenPalette={() => setPalette("all")}
              onQueue={onQueue}
              queueAct={queueAct}
              prefill={prefill}
              open={open}
              deleted={deleted}
              clear={clear}
              onConversation={setChatSession}
              active={view === "chat"}
              remote={Boolean(caps.remote)}
              grokSignIn={Boolean(caps.grokSignIn)}
              side={
                <ChatSide
                  active={view === "chat"}
                  conversationId={chatSession}
                  pulse={Boolean(caps.pulse)}
                  onOpen={openView}
                />
              }
            />
          </div>
          <div hidden={view !== "automations"}>
            <Automations
              active={view === "automations"}
              onStarted={(id, name) =>
                setActivity((now) => ({
                  ...pruneActivity(now, Date.now()),
                  [`run:${id}`]: routineStarted(id, name, Date.now()),
                }))
              }
              onFinished={(run) => {
                notifyRun(run);
                setActivity((now) => {
                  const key = `run:${run.id}`;
                  const item = now[key] ?? routineStarted(run.id, run.name, Date.now());
                  return {
                    ...pruneActivity(now, Date.now()),
                    [key]: routineFinished(item, run.ok, run.text, Date.now()),
                  };
                });
              }}
            />
          </div>
          {/* Chat and Automations are not in <Activity>: hidden there, their
              listeners would stop and a reply landing meanwhile would be lost. */}
          {opened.has("home") && (
            <Page shown={view === "home"}>
              <Home
                conversationId={openChat}
                remote={Boolean(caps.remote)}
                onOpen={openView}
                onOpenChat={openSaved}
                onAsk={ask}
                onSettings={(section) => run({ kind: "settings", section })}
              />
            </Page>
          )}
          {opened.has("objectives") && (
            <Page shown={view === "objectives"}>
              <Objectives
                conversationId={openChat}
                remote={Boolean(caps.remote)}
                onOpenChat={openSaved}
                onHome={() => setView("home")}
                onAsk={ask}
              />
            </Page>
          )}
          {opened.has("pulse") && caps.pulse && (
            <Page shown={view === "pulse"}>
              <Pulse onRun={ask} onOpenInterests={() => run({ kind: "settings", section: "pulse" })} />
            </Page>
          )}
          {opened.has("workboard") && (
            <Page shown={view === "workboard"}>
              <Workboard
                onWork={ask}
                resumable={!caps.remote}
                openCard={openCard}
                onCards={setCardChats}
                onSwept={noteSweep}
                onResume={({ prompt, conversationId }) => {
                  // The resume block goes once, as the resume turn's input.
                  setPrefill((now) => ({ text: prompt, n: (now?.n ?? 0) + 1, send: true, conversationId }));
                  setView("chat");
                }}
              />
            </Page>
          )}
          {/* Code hides its own pages: its agent window stays mounted like Chat. */}
          {opened.has("code") && !caps.remote && (
            <PageBoundary shown={view === "code"}>
              <Suspense fallback={null}>
                <Code
                  shown={view === "code"}
                  onOpenSettings={() => setView("settings")}
                  onSignIn={() => run({ kind: "settings", section: "account" })}
                  onOpenView={openView}
                  onOpenPalette={() => setPalette("all")}
                  grokSignIn={Boolean(caps.grokSignIn)}
                  chatSession={chatSession}
                  onShowChat={() => setView("chat")}
                  onSession={onAgentSession}
                  deleted={deleted}
                />
              </Suspense>
            </PageBoundary>
          )}
          {opened.has("command") && !caps.remote && (
            <Page shown={view === "command"}>
              <CommandBox
                onRun={askNow}
              />
            </Page>
          )}
          {opened.has("journal") && !caps.remote && (
            <Page shown={view === "journal"}>
              <Journal
                onMission={({ prompt, conversationId }) => {
                  setPrefill((now) => ({ text: prompt, n: (now?.n ?? 0) + 1, conversationId }));
                  setView("chat");
                }}
              />
            </Page>
          )}
          {opened.has("history") && !caps.remote && (
            <Page shown={view === "history"}>
              <History
                onOpen={openSaved}
                onDeleted={(id) =>
                  setDeleted((now) => ({ id, ids: [...(now?.ids ?? []), id], n: (now?.n ?? 0) + 1 }))
                }
              />
            </Page>
          )}
          {opened.has("memory") && (
            <Page shown={view === "memory"}>
              <Memory />
            </Page>
          )}
          {opened.has("skills") && (
            <Page shown={view === "skills"}>
              <Skills onUse={ask} onOpenConnectors={() => setView("connectors")} remote={Boolean(caps.remote)} />
            </Page>
          )}
          {opened.has("connectors") && (
            <Page shown={view === "connectors"}>
              <Connectors remote={Boolean(caps.remote)} />
            </Page>
          )}
          {opened.has("updates") && !caps.remote && (
            <Page shown={view === "updates"}>
              <UpdatesDrivers />
            </Page>
          )}
          {opened.has("library") && caps.library && (
            <Page shown={view === "library"}>
              <Library />
            </Page>
          )}
          {opened.has("mods") && caps.mods && !caps.remote && (
            <Page shown={view === "mods"}>
              <Mods />
            </Page>
          )}
          {opened.has("settings") && (
            <Page shown={view === "settings"}>
              <Settings
                caps={caps}
                goTo={goTo}
                onClose={() => setView(before)}
                backTo={VIEW_LABELS[before]}
              />
            </Page>
          )}
          {opened.has("devices") && caps.webAccess && !caps.remote && (
            <Page shown={view === "devices"}>
              <Devices onOpenWebAccess={() => run({ kind: "settings", section: "webAccess" })} onRun={askNow} />
            </Page>
          )}
          {opened.has("imagine") && (
            <Page shown={view === "imagine"}>
              <Imagine />
            </Page>
          )}
          {opened.has("agents") && (
            <Page shown={view === "agents"}>
              <Agents
                conversationId={openChat}
                onOpenChat={() => setView("chat")}
                onRun={(prompt, threadId) => runQueued(prompt, threadId)}
              />
            </Page>
          )}
        </main>
      </div>
    </div>
  );
}
