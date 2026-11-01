import { Activity, lazy, Suspense, useEffect, useRef, useState, type ReactNode } from "react";
import { CORE_ONLY, getCapabilities, getSettings, getWorkboard, onChatEvent, type Capabilities } from "./api";
import { cardsByChat, chatActivity, pruneActivity, routineFinished, routineStarted, type ActivityItem } from "./activity";
import { ActivityDock } from "./ActivityDock";
import { Automations, type RunNotice } from "./Automations";
import { Chat, newConversationId, type Prefill } from "./Chat";
import { NO_SOURCES, Palette, VIEW_LABELS, views, type PaletteAction, type View } from "./Palette";
import { COMPOSER_SLASH, shellKeys, type CommandRunners } from "./paletteCommands";
import { Shortcuts } from "./Shortcuts";
import type { Section } from "./sections";
import { useOverscroll } from "./rubberBand";
import { useHidingBar } from "./tabBar";
import { applyTheme } from "./theme";
import { LOGO_MARK_URL, PRODUCT_NAME } from "./brand";
import { TabIcon } from "./TabIcon";

// Chat and Automations stay mounted, so they load up front. The other screens
// load the first time they open, and again in the background once the app is
// idle so that first click does not wait on the network or disk.
const loaders = {
  connectors: () => import("./Connectors").then((m) => ({ default: m.Connectors })),
  journal: () => import("./Journal").then((m) => ({ default: m.Journal })),
  workboard: () => import("./Workboard").then((m) => ({ default: m.Workboard })),
  library: () => import("./Library").then((m) => ({ default: m.Library })),
  memory: () => import("./Memory").then((m) => ({ default: m.Memory })),
  pulse: () => import("./Pulse").then((m) => ({ default: m.Pulse })),
  settings: () => import("./Settings").then((m) => ({ default: m.Settings })),
};
const Connectors = lazy(loaders.connectors);
const Journal = lazy(loaders.journal);
const Library = lazy(loaders.library);
const Memory = lazy(loaders.memory);
const Pulse = lazy(loaders.pulse);
const Settings = lazy(loaders.settings);
const Workboard = lazy(loaders.workboard);

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
  const [activity, setActivity] = useState<Record<string, ActivityItem>>({});
  // Which work card each chat works, from the Workboard's last load.
  const [cardChats, setCardChats] = useState<Record<string, string>>({});
  const [openCard, setOpenCard] = useState<{ id: string; n: number }>();
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
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
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
  }, []);

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
    "sign-in": () => run({ kind: "settings", section: "account" }),
    shortcuts: () => setSheet(true),
  };

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
          {views(caps).map((name) => (
            <button
              key={name}
              type="button"
              aria-pressed={view === name}
              title={VIEW_LABELS[name]}
              onClick={() => setView(name)}
            >
              <TabIcon view={name} />
              <span className="tab-label">{VIEW_LABELS[name]}</span>
            </button>
          ))}
        </nav>
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
      <div hidden={view !== "chat"}>
        <Chat
          onOpenSettings={() => setView("settings")}
          onSignIn={() => run({ kind: "settings", section: "account" })}
          onOpenAutomations={() => setView("automations")}
          onOpenView={setView}
          prefill={prefill}
          notice={notice}
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
      {opened.has("pulse") && caps.pulse && (
        <Page shown={view === "pulse"}>
          <Pulse onRun={ask} />
        </Page>
      )}
      {opened.has("workboard") && (
        <Page shown={view === "workboard"}>
          <Workboard
            readOnly={Boolean(caps.remote)}
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
      {opened.has("memory") && (
        <Page shown={view === "memory"}>
          <Memory />
        </Page>
      )}
      {opened.has("connectors") && (
        <Page shown={view === "connectors"}>
          <Connectors />
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
    </div>
  );
}
