import { useEffect, useState } from "react";
import { CORE_ONLY, getCapabilities, type Capabilities } from "./api";
import { Automations } from "./Automations";
import { Chat, type Prefill } from "./Chat";
import { Connectors } from "./Connectors";
import { Journal } from "./Journal";
import { Memory } from "./Memory";
import { Palette, VIEW_LABELS, views, type PaletteAction, type View } from "./Palette";
import { Pulse } from "./Pulse";
import type { Section } from "./sections";
import { Settings } from "./Settings";

export function App() {
  const [view, setView] = useState<View>("chat");
  const [prefill, setPrefill] = useState<Prefill>();
  const [caps, setCaps] = useState<Capabilities>(CORE_ONLY);
  const [palette, setPalette] = useState(false);
  const [goTo, setGoTo] = useState<{ section: Section; n: number }>();
  useEffect(() => {
    getCapabilities()
      .catch(() => CORE_ONLY)
      .then(setCaps);
  }, []);
  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setPalette((open) => !open);
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  function ask(text: string) {
    setPrefill((now) => ({ text, n: (now?.n ?? 0) + 1 }));
    setView("chat");
  }

  function run(action: PaletteAction) {
    if (action.kind === "view") setView(action.view);
    else if (action.kind === "ask") ask(action.text);
    else {
      setGoTo((now) => ({ section: action.section, n: (now?.n ?? 0) + 1 }));
      setView("settings");
    }
  }
  return (
    <div className="app">
      <header className="topbar">
        <nav className="tabs" aria-label="Views">
          {views(caps).map((name) => (
            <button
              key={name}
              type="button"
              aria-pressed={view === name}
              onClick={() => setView(name)}
            >
              {VIEW_LABELS[name]}
            </button>
          ))}
        </nav>
        <button
          type="button"
          className="palette-open"
          title="Command palette (Ctrl+K)"
          onClick={() => setPalette(true)}
        >
          Command
        </button>
      </header>
      {palette && <Palette caps={caps} onPick={run} onClose={() => setPalette(false)} />}
      {/* Chat and Automations stay mounted so a reply or a Run now in flight
          is not lost when another tab opens. */}
      <div hidden={view !== "chat"}>
        <Chat
          onOpenSettings={() => setView("settings")}
          prefill={prefill}
          active={view === "chat"}
          remote={Boolean(caps.remote)}
        />
      </div>
      <div hidden={view !== "automations"}>
        <Automations active={view === "automations"} />
      </div>
      {view === "pulse" && caps.pulse && (
        <Pulse
          onRun={ask}
        />
      )}
      {view === "journal" && !caps.remote && (
        <Journal
          onMission={({ prompt, conversationId }) => {
            setPrefill((now) => ({ text: prompt, n: (now?.n ?? 0) + 1, conversationId }));
            setView("chat");
          }}
        />
      )}
      {view === "memory" && <Memory />}
      {view === "connectors" && <Connectors />}
      {view === "settings" && <Settings caps={caps} goTo={goTo} />}
    </div>
  );
}
