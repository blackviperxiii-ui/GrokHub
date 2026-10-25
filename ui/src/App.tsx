import { useEffect, useState } from "react";
import { CORE_ONLY, getCapabilities, type Capabilities } from "./api";
import { Automations } from "./Automations";
import { Chat } from "./Chat";
import { Connectors } from "./Connectors";
import { Memory } from "./Memory";
import { Pulse } from "./Pulse";
import { Settings } from "./Settings";

type View = "chat" | "pulse" | "automations" | "memory" | "connectors" | "settings";

export function App() {
  const [view, setView] = useState<View>("chat");
  const [prefill, setPrefill] = useState<{ text: string; n: number }>();
  const [caps, setCaps] = useState<Capabilities>(CORE_ONLY);
  useEffect(() => {
    getCapabilities()
      .catch(() => CORE_ONLY)
      .then(setCaps);
  }, []);
  return (
    <div className="app">
      <nav className="tabs" aria-label="Views">
        <button type="button" aria-pressed={view === "chat"} onClick={() => setView("chat")}>
          Chat
        </button>
        {caps.pulse && (
          <button type="button" aria-pressed={view === "pulse"} onClick={() => setView("pulse")}>
            Pulse
          </button>
        )}
        <button
          type="button"
          aria-pressed={view === "automations"}
          onClick={() => setView("automations")}
        >
          Automations
        </button>
        <button type="button" aria-pressed={view === "memory"} onClick={() => setView("memory")}>
          Memory
        </button>
        <button
          type="button"
          aria-pressed={view === "connectors"}
          onClick={() => setView("connectors")}
        >
          Connectors
        </button>
        <button
          type="button"
          aria-pressed={view === "settings"}
          onClick={() => setView("settings")}
        >
          Settings
        </button>
      </nav>
      {/* Chat and Automations stay mounted so a reply or a Run now in flight
          is not lost when another tab opens. */}
      <div hidden={view !== "chat"}>
        <Chat onOpenSettings={() => setView("settings")} prefill={prefill} />
      </div>
      <div hidden={view !== "automations"}>
        <Automations active={view === "automations"} />
      </div>
      {view === "pulse" && caps.pulse && (
        <Pulse
          onRun={(text) => {
            setPrefill((now) => ({ text, n: (now?.n ?? 0) + 1 }));
            setView("chat");
          }}
        />
      )}
      {view === "memory" && <Memory />}
      {view === "connectors" && <Connectors />}
      {view === "settings" && <Settings caps={caps} />}
    </div>
  );
}
