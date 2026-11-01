import { Activity, lazy, Suspense, useCallback, useEffect, useRef, useState, type ReactNode } from "react";
import {
  CORE_ONLY,
  getCapabilities,
  getSettings,
  getWorkboard,
  haltDesktop,
  onAutomationRan,
  onChatEvent,
  onHalt,
  shareSchedulerClock,
  type Capabilities,
} from "./api";
import { cardsByChat, chatActivity, pruneActivity, routineFinished, routineStarted, type ActivityItem } from "./activity";
import { ActivityDock } from "./ActivityDock";
import { Automations, resultLine, type RunNotice } from "./Automations";
import { Chat, newConversationId, type Prefill } from "./Chat";
import { HALT_DONE, HALT_FAILED, HaltButton, HaltNote, haltShown, isHaltKey } from "./Halt";
import { NO_SOURCES, Palette, VIEW_LABELS, views, type PaletteAction, type View } from "./Palette";
import { COMPOSER_SLASH, shellKeys, type CommandRunners } from "./paletteCommands";
import { Shortcuts } from "./Shortcuts";
import type { Section } from "./sections";
import { useOverscroll } from "./rubberBand";
import { useHidingBar } from "./tabBar";
import { applyTheme } from "./theme";
import { LOGO_MARK_URL, PRODUCT_NAME } from "./brand";
import { TabIcon } from "./TabIcon";
import { ChatTabMenu, ClearChatDialog } from "./ClearChat";

// Chat and Automations stay mounted, so they load up front. The other screens
// load the first time they open, and again in the background once the app is
// idle so that first click does not wait on the network or disk.
const loaders = {
  agents: () => import("./Agents").then((m) => ({ default: m.Agents })),
  connectors: () => import("./Connectors").then((m) => ({ default: m.Connectors })),
  imagine: () => import("./Imagine").then((m) => ({ default: m.Imagine })),
  devices: () => import("./Devices").then((m) => ({ default: m.Devices })),
  journal: () => import("./Journal").then((m) => ({ default: m.Journal })),
  workboard: () => import("./Workboard").then((m) => ({ default: m.Workboard })),
  history: () => import("./History").then((m) => ({ default: m.History })),
  home: () => import("./Home").then((m) => ({ default: m.Home })),
  library: () => import("./Library").then((m) => ({ default: m.Library })),
  memory: () => import("./Memory").then((m) => ({ default: m.Memory })),
  objectives: () => import("./Objectives").then((m) => ({ default: m.Objectives })),
  skills: () => import("./Skills").then((m) => ({ default: m.Skills })),
  pulse: () => import("./Pulse").then((m) => ({ default: m.Pulse })),
  settings: () => import("./Settings").then((m) => ({ default: m.Settings })),
};
const Agents = lazy(loaders.agents);
const Connectors = lazy(loaders.connectors);
const Imagine = lazy(loaders.imagine);
const Devices = lazy(loaders.devices);
const History = lazy(loaders.history);
const Home = lazy(loaders.home);
const Journal = lazy(loaders.journal);
const Library = lazy(loaders.library);
const Memory = lazy(loaders.memory);
const Objectives = lazy(loaders.objectives);
const Pulse = lazy(loaders.pulse);
const Skills = lazy(loaders.skills);
const Settings = lazy(loaders.settings);
const Workboard = lazy(loaders.workboard);

/** How often the window tells the scheduler its clock. */
const CLOCK_SHARE_MS = 5 * 60 * 1000;

function preloadViews() {
  for (const load of Object.values(loaders)) void load().catch(() => {});
}

/** A page that stays mounted once it has opened. Hidden, it keeps its state
 * (a half-typed form stays) and its effects stop; they run again when it
 * shows, so its lists reload. */
function Page({ shown, children }: { shown: boolean; children: ReactNode }) {
  return (
    <Activity mode={shown ? "visible" : "hidden"}>
      <Suspense fallback={null}>{children}</Suspense>
    </Activity>
  );
}

export function App() {
  const [view, setView] = useState<View>("chat");
  const [prefill, setPrefill] = useState<Prefill>();
  const [caps, setCaps] = useState<Capabilities>(CORE_ONLY);
  const [palette, setPalette] = useState(false);
  const [sheet, setSheet] = useState(false);
  const [signedOut, setSignedOut] = useState(false);
  const [goTo, setGoTo] = useState<{ section: Section; n: number }>();
  const [notice, setNotice] = useState<RunNotice & { n: number }>();
  const [open, setOpen] = useState<{ id: string; n: number }>();
  const [deleted, setDeleted] = useState<{ id: string; n: number }>();
  /** The Chat tab's right-click menu, at the pointer; then its confirm; then the clear. */
  const [tabMenu, setTabMenu] = useState<{ x: number; y: number } | null>(null);
  const [askClear, setAskClear] = useState(false);
  const [clear, setClear] = useState<{ n: number }>();
  const [activity, setActivity] = useState<Record<string, ActivityItem>>({});
  // Which work card each chat works, from the Workboard's last load.
  const [cardChats, setCardChats] = useState<Record<string, string>>({});
  const [openCard, setOpenCard] = useState<{ id: string; n: number }>();
  const [desktopOn, setDesktopOn] = useState(false);
  const [haltNote, setHaltNote] = useState<string>();
  // Halt is the computer's own: phones and the web app have no desktop control.
  const canHalt = caps.desktopControl && !caps.remote;
  const busy = Object.values(activity).some((item) => item.status === "running" || item.status === "waiting");
  // The open chat for the Agents page: the conversation the window last heard from.
  const [openChat, setOpenChat] = useState<string>();
  // The pages opened so far. Each mounts the first time it opens, not before.
  const [opened, setOpened] = useState<ReadonlySet<View>>(() => new Set(["chat"]));
  if (!opened.has(view)) setOpened(new Set(opened).add(view));
  const barAway = useHidingBar(palette);
  // Closing Settings goes back to the screen that was open before it.
  const before = useRef<View>("chat");
  useEffect(() => {
    if (view !== "settings") before.current = view;
  }, [view]);
  useOverscroll();
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
      (board) => setCardChats(cardsByChat(board)),
      () => {},
    );
  }, []);
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
    if (palette) getSettings().then((s) => setSignedOut(!s.hasApiKey && !s.signedIn), () => {});
  }, [palette]);
  // The Activity Dock follows every chat turn, whichever view is open.
  useEffect(() => {
    let stop: (() => void) | undefined;
    let dropped = false;
    void onChatEvent((envelope) => {
      setOpenChat(envelope.conversationId);
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
  }, []);
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
  // A scheduled run ended: chat says so and the Activity Dock shows it.
  useEffect(() => {
    let stop: (() => void) | undefined;
    let dropped = false;
    void onAutomationRan(({ id, name, result }) => {
      const run: RunNotice = { id, name, ok: !result.error, text: resultLine(result) };
      setNotice((now) => ({ ...run, n: (now?.n ?? 0) + 1 }));
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
  }, []);
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
        setPalette((open) => !open);
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

  function ask(text: string) {
    setPrefill((now) => ({ text, n: (now?.n ?? 0) + 1 }));
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
  return (
    <div className={barAway ? "app bar-away" : "app"}>
      <header className="topbar">
        <div className="brand-mark">
          <img src={LOGO_MARK_URL} alt="" width={22} height={22} />
          <span>{PRODUCT_NAME}</span>
        </div>
        <nav className="tabs" aria-label="Views">
          {tabMenu && (
            <ChatTabMenu at={tabMenu} onClear={() => setAskClear(true)} onClose={() => setTabMenu(null)} />
          )}
          {views(caps).map((name) => (
            <button
              key={name}
              type="button"
              aria-pressed={view === name}
              title={VIEW_LABELS[name]}
              onClick={() => setView(name)}
              onContextMenu={
                name === "chat" && !caps.remote
                  ? (event) => {
                      event.preventDefault();
                      setTabMenu({ x: event.clientX, y: event.clientY });
                    }
                  : undefined
              }
            >
              <TabIcon view={name} />
              <span className="tab-label">{VIEW_LABELS[name]}</span>
            </button>
          ))}
        </nav>
        {canHalt && haltShown(desktopOn, busy) && <HaltButton onHalt={halt} />}
        <ActivityDock
          items={Object.values(activity)}
          onOpenChat={() => setView("chat")}
          onOpenAutomations={() => setView("automations")}
          cardFor={(chat) => cardChats[chat]}
          onOpenCard={(id) => {
            setOpenCard((now) => ({ id, n: (now?.n ?? 0) + 1 }));
            setView("workboard");
          }}
        />
        <button
          type="button"
          className="palette-open"
          title="Command palette (Ctrl+K)"
          onClick={() => setPalette(true)}
        >
          Command
        </button>
      </header>
      {haltNote && <HaltNote text={haltNote} onDismiss={() => setHaltNote(undefined)} />}
      {palette && (
        <Palette
          caps={caps}
          sources={{ ...NO_SOURCES, runners, signedOut, slash: COMPOSER_SLASH }}
          onPick={run}
          onClose={() => setPalette(false)}
        />
      )}
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
          onOpenAutomations={() => setView("automations")}
          onOpenView={setView}
          prefill={prefill}
          notice={notice}
          open={open}
          deleted={deleted}
          clear={clear}
          active={view === "chat"}
          remote={Boolean(caps.remote)}
          grokSignIn={Boolean(caps.grokSignIn)}
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
            setNotice((now) => ({ ...run, n: (now?.n ?? 0) + 1 }));
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
            onOpen={setView}
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
          />
        </Page>
      )}
      {opened.has("pulse") && caps.pulse && (
        <Page shown={view === "pulse"}>
          <Pulse onRun={ask} />
        </Page>
      )}
      {opened.has("workboard") && (
        <Page shown={view === "workboard"}>
          <Workboard
            onWork={ask}
            resumable={!caps.remote}
            openCard={openCard}
            onCards={setCardChats}
            onResume={({ prompt, conversationId }) => {
              setPrefill((now) => ({ text: prompt, n: (now?.n ?? 0) + 1, conversationId }));
              setView("chat");
            }}
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
            onDeleted={(id) => setDeleted((now) => ({ id, n: (now?.n ?? 0) + 1 }))}
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
      {opened.has("library") && caps.library && (
        <Page shown={view === "library"}>
          <Library />
        </Page>
      )}
      {opened.has("settings") && (
        <Page shown={view === "settings"}>
          <Settings caps={caps} goTo={goTo} onClose={() => setView(before.current)} />
        </Page>
      )}
      {opened.has("devices") && caps.webAccess && !caps.remote && (
        <Page shown={view === "devices"}>
          <Devices onOpenWebAccess={() => run({ kind: "settings", section: "webAccess" })} />
        </Page>
      )}
      {opened.has("imagine") && (
        <Page shown={view === "imagine"}>
          <Imagine />
        </Page>
      )}
      {opened.has("agents") && (
        <Page shown={view === "agents"}>
          <Agents conversationId={openChat} onOpenChat={() => setView("chat")} />
        </Page>
      )}
    </div>
  );
}
