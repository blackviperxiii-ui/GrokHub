import { useState } from "react";
import { Automations } from "./Automations";
import { Chat } from "./Chat";
import { Settings } from "./Settings";

type View = "chat" | "automations" | "settings";

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
        <button
          type="button"
          aria-pressed={view === "settings"}
          onClick={() => setView("settings")}
        >
          Settings
        </button>
      </nav>
      {/* Chat stays mounted so a reply keeps streaming while Settings is open. */}
      <div hidden={view !== "chat"}>
        <Chat onOpenSettings={() => setView("settings")} />
      </div>
      {view === "automations" && <Automations />}
      {view === "settings" && <Settings />}
    </div>
  );
}
