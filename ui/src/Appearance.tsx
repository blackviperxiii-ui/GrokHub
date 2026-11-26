import { useEffect, useState } from "react";
import { setTheme, type Theme } from "./api";
import { LookRows } from "./AutoSetup";
import { PRODUCT_NAME } from "./brand";
import { loadRailPrefs, RAIL_PREFS_EVENT, saveRailPrefs, type RailMode, type RailPrefs } from "./railPrefs";
import { applyTheme } from "./theme";

export const APPEARANCE_NOTE = `System follows your computer. Ops Dark, OLED Black and Soft Charcoal are the ${PRODUCT_NAME} dark themes (red primary, amber for the one call-to-action).`;
export const SIDEBAR_NOTE =
  "Simple keeps Home, Chat, Code, Pulse, Library and Settings on the rail. Advanced shows every page in grouped sections. Right-click a page on the rail to pin or unpin it.";

const CHOICES: { value: Theme; label: string }[] = [
  { value: "system", label: "System" },
  { value: "ops-dark", label: "Ops Dark" },
  { value: "oled-black", label: "OLED Black" },
  { value: "soft-charcoal", label: "Soft Charcoal" },
  { value: "light", label: "Light" },
];

/** Settings → Appearance. A pick shows at once and is saved in shell.json. */
function displayTheme(theme: Theme): Theme {
  return theme === "dark" ? "ops-dark" : theme;
}

export function Appearance({
  theme,
  textSize = false,
  onSaved,
}: {
  theme: Theme;
  /** Text size, reduce motion and Objectives beside chat (auto setup's), on a desktop. */
  textSize?: boolean;
  onSaved: (theme: Theme) => void;
}) {
  const shown = displayTheme(theme);
  const [note, setNote] = useState<string | null>(null);
  const [mode, setMode] = useState<RailMode>(() => loadRailPrefs().mode);

  useEffect(() => {
    function onPrefs(event: Event) {
      const detail = (event as CustomEvent<RailPrefs>).detail;
      if (detail?.mode) setMode(detail.mode);
    }
    window.addEventListener(RAIL_PREFS_EVENT, onPrefs);
    return () => window.removeEventListener(RAIL_PREFS_EVENT, onPrefs);
  }, []);

  async function choose(next: Theme) {
    applyTheme(next);
    try {
      const saved = await setTheme(next);
      onSaved(saved.theme);
      setNote(null);
    } catch (err) {
      applyTheme(theme);
      setNote(String(err));
    }
  }

  function chooseMode(next: RailMode) {
    saveRailPrefs({ ...loadRailPrefs(), mode: next });
    setMode(next);
  }

  return (
    <div className="appearance" role="group" aria-label="Appearance">
      <p className="note">{APPEARANCE_NOTE}</p>
      <div className="chip-group" role="group" aria-label="Theme">
        {CHOICES.map((c) => (
          <button
            key={c.value}
            type="button"
            className="chip"
            aria-pressed={shown === c.value}
            onClick={() => void choose(c.value)}
          >
            <span className="theme-swatch" data-swatch={c.value} aria-hidden="true">
              <i />
              <i />
            </span>
            {c.label}
          </button>
        ))}
      </div>
      <p className="note">{SIDEBAR_NOTE}</p>
      <div className="chip-group" role="group" aria-label="Sidebar">
        <button type="button" className="chip" aria-pressed={mode === "simple"} onClick={() => chooseMode("simple")}>
          Simple
        </button>
        <button
          type="button"
          className="chip"
          aria-pressed={mode === "advanced"}
          onClick={() => chooseMode("advanced")}
        >
          Advanced
        </button>
      </div>
      {note && <p className="note warn">{note}</p>}
      {textSize && <LookRows />}
    </div>
  );
}
