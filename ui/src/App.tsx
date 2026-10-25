import { useState } from "react";
import { Automations } from "./Automations";
import { Chat } from "./Chat";
import { Memory } from "./Memory";
import { Settings } from "./Settings";

type View = "chat" | "automations" | "memory" | "settings";

export function App() {
  const [view, setView] = useState<View>("chat");
  return (
    <div className="app">
      <nav className="tabs" aria-label="Views">
        <button type="button" aria-pressed={view === "chat"} onClick={() => setView("chat")}>
          Chat
        </button>
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
          aria-pressed={view === "settings"}
          onClick={() => setView("settings")}
        >
          Settings
        </button>
      </nav>
      {/* Chat and Automations stay mounted so a reply or a Run now in flight
          is not lost when another tab opens. */}
      <div hidden={view !== "chat"}>
        <Chat onOpenSettings={() => setView("settings")} />
      </div>
      <div hidden={view !== "automations"}>
        <Automations active={view === "automations"} />
      </div>
      {view === "memory" && <Memory />}
      {view === "settings" && <Settings />}
    </div>
  );
}
