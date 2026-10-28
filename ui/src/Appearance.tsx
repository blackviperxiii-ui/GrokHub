import { useState } from "react";
import { setTheme, type Theme } from "./api";
import { PRODUCT_NAME } from "./brand";
import { applyTheme } from "./theme";

export const APPEARANCE_NOTE = `System follows your computer. Ops Dark, OLED Black and Soft Charcoal match the ${PRODUCT_NAME} cabin.`;

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

export function Appearance({ theme, onSaved }: { theme: Theme; onSaved: (theme: Theme) => void }) {
  const shown = displayTheme(theme);
  const [note, setNote] = useState<string | null>(null);

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
            {c.label}
          </button>
        ))}
      </div>
      {note && <p className="note warn">{note}</p>}
    </div>
  );
}
